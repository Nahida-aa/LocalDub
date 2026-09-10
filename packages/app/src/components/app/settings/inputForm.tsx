//! 全局输入参数表单: 以表单编辑仓库根 input.jsonc 的常用字段。
//!
//! 与 `input.jsonc` 文本 tab 并存: 表单适合快速改常用字段, 冷门/未覆盖字段
//! (各 stage 的详细参数) 仍走文本编辑。
//!
//! 保存语义: 按字段 merge 回解析出的对象再整体重写文件 —— **文件内的注释会丢失**,
//! 因此保存前自动备份到 `input.jsonc.bak`。
//!
//! 表单状态由 `useAppForm` (tanstack/solid-form) 管理, 字段 UI 以 CardX 渲染
//! (form.Field render prop 桥接 field API 与组件)。

import { createEffect, Show } from "solid-js";
import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { Input } from "@repo/ui-solid/base/input";
import { Button } from "@repo/ui-solid/base/button";
import { CardX } from "@repo/ui-solid/custom/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@repo/ui-solid/base/select";
import { toastError } from "@repo/ui-solid/custom/toast";
import { client } from "#/integrations/fnrpc/client.ts";
import { useAppForm } from "@repo/ui-solid/form/useAppForm";

const INPUT_PATH = "input.jsonc";

import type {
  Pipeline,
  ServerAction,
  ServerType_Serialize,
  StageName,
  SubtitleSource,
  TaskAction,
} from "@repo/sdk/fnrpc/bindings";
import { ScrollArea } from "@repo/ui-solid/base/scroll-area";

const keysOf = <T extends Record<string, unknown>>(o: T): string[] => Object.keys(o);

// 下拉选项从 Rust 枚举派生 (specta -> bindings 类型): satisfies 全键约束保证
// Rust 枚举加/改值 -> gen-ts-sdk -> 此处编译报错, 强制同步 (不再手写漏值)。

// InputCommand 无 specta 类型, 保持手写
const COMMANDS = ["task", "servers", "env", "cookie"];

const TASK_ACTIONS = keysOf({
  start: "",
  continue: "",
  import: "",
  enqueue_start: "",
  enqueue_continue: "",
  enqueue_import: "",
  list_queue: "",
  cancel_queue: "",
  status: "",
  get_group_list: "",
  get_task_ctx: "",
  generate_meta: "",
} satisfies Record<TaskAction, string>);

const STAGES = [
  "",
  ...keysOf({
    separate: "",
    separate_after: "",
    asr: "",
    asr_fix: "",
    sf_ocr_pre: "",
    sf_ocr: "",
    sf_ocr_fix: "",
    asr_ocr_pre: "",
    asr_ocr: "",
    asr_ocr_fix: "",
    translate: "",
    split_audio: "",
    tts: "",
    mix_audio: "",
    mix_video: "",
  } satisfies Record<StageName, string>),
];

const PIPELINES = ["", ...keysOf({ dub: "", subtitle: "" } satisfies Record<Pipeline, string>)];

const SUBTITLE_SOURCES = [
  "",
  ...keysOf({ asr: "", sf_ocr: "", asr_ocr: "" } satisfies Record<SubtitleSource, string>),
];

const SERVER_ACTIONS = [
  "",
  ...keysOf({ status: "", start: "", stop: "", discovery: "" } satisfies Record<
    ServerAction,
    string
  >),
];

const SERVER_NAMES = [
  "",
  ...keysOf({
    main: "",
    voxcpm_torch_gradio: "",
  } satisfies Record<ServerType_Serialize, string>),
];

type FormState = {
  command: string;
  action: string;
  url: string;
  taskDir: string;
  continueFrom: string;
  targetStage: string;
  pipeline: string;
  subtitleSource: string;
  sourceLang: string;
  targetLang: string;
  serverAction: string;
  serverName: string;
};

const emptyForm = (): FormState => ({
  command: "task",
  action: "",
  url: "",
  taskDir: "",
  continueFrom: "",
  targetStage: "",
  pipeline: "",
  subtitleSource: "",
  sourceLang: "",
  targetLang: "",
  serverAction: "",
  serverName: "",
});

/// jsonc 容错解析: 去掉字符串外的 // 与 /* */ 注释及尾逗号, 便于 JSON.parse。
/// (字符串内的 // 必须保留, 否则 http:// 会被误伤)
function stripJsonc(text: string): string {
  let out = "";
  let inStr = false;
  let quote = '"';
  let escaped = false;
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    const next = text[i + 1];
    if (inStr) {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) inStr = false;
      i += 1;
      continue;
    }
    if (c === '"' || c === "'") {
      inStr = true;
      quote = c;
      out += c;
      i += 1;
      continue;
    }
    if (c === "/" && next === "/") {
      while (i < text.length && text[i] !== "\n") i += 1;
      continue;
    }
    if (c === "/" && next === "*") {
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) i += 1;
      i += 2;
      continue;
    }
    out += c;
    i += 1;
  }
  return out.replace(/,(\s*[}\]])/g, "$1");
}

function parseJsonc(raw: string): Record<string, any> {
  try {
    return JSON.parse(stripJsonc(raw)) as Record<string, any>;
  } catch {
    return {};
  }
}

type Option = { value: string; label: string };

/// 空值哨兵: Kobalte 把空字符串当"无选中值" (selectedOption() 为 undefined),
/// 需要用一个非空标记表示"未设置", 提交时再映射回空串。
const EMPTY = "__none__";

/// form.Field render prop 的 field API 最小面 (value + handleChange)。
type FieldLike = {
  state: { value: string | undefined };
  handleChange: (v: string) => void;
};

/// 下拉字段卡片 (与 useAppForm field 桥接)。
function CardSelect(props: {
  title: string;
  description: string;
  field: () => FieldLike;
  options: string[];
}) {
  // Solid: JSX 属性需是"调用"才会编译成 getter —— 直接写对象字面量会被静态化,
  // signal 变化时选中项不同步 (React 靠重渲染天然正确, Solid 不行)。
  const selected = (): Option => ({
    value: props.field().state.value || EMPTY,
    label: props.field().state.value || "—",
  });
  const options = (): Option[] =>
    props.options.map((v) => ({ value: v || EMPTY, label: v || "—" }));

  return (
    <CardX
      title={props.title}
      description={props.description}
      size="sm"
      Footer={
        <Select<Option>
          value={selected()}
          optionValue="value"
          optionTextValue="label"
          onChange={(v) => {
            const raw = v?.value ?? EMPTY;
            props.field().handleChange(raw === EMPTY ? "" : raw);
          }}
          options={options()}
          itemComponent={(p) => <SelectItem item={p.item}>{p.item.rawValue.label}</SelectItem>}
        >
          <SelectTrigger class="w-45">
            <SelectValue<Option>>{(state) => state.selectedOption()?.label ?? "—"}</SelectValue>
          </SelectTrigger>
          <SelectContent />
        </Select>
      }
    />
  );
}

/// 文本字段卡片 (与 useAppForm field 桥接)。
function CardInput(props: {
  title: string;
  description: string;
  field: () => FieldLike;
  placeholder?: string;
}) {
  return (
    <CardX
      title={props.title}
      description={props.description}
      size="sm"
      Footer={
        <Input
          class="w-80"
          value={props.field().state.value ?? ""}
          placeholder={props.placeholder ?? ""}
          onInput={(e) => props.field().handleChange(e.currentTarget.value)}
        />
      }
    />
  );
}

export function InputFormSettings() {
  const fileQ = useQuery(() => client.read_app_file_text.queryOptions(INPUT_PATH));
  const writeMut = useMutation(() => client.write_app_file_text.mutationOptions());
  const qc = useQueryClient();
  // 只在首次读到内容时填充: 保存后 invalidate / 窗口重取会再次触发 effect,
  // 不能覆盖用户未保存的编辑。
  let filled = false;

  // form 闭包内引用自身 (onSubmit 末尾 reset), 提交时机变量已定义。
  const form = useAppForm(() => ({
    defaultValues: emptyForm(),
    onSubmit: async ({ value }) => {
      const raw = fileQ.data ?? "";
      const prev = parseJsonc(raw);
      const merged: Record<string, any> = {
        ...prev,
        command: value.command,
        task: {
          ...((prev.task ?? {}) as Record<string, any>),
          action: value.action || undefined,
          url: value.url || undefined,
          taskDir: value.taskDir || undefined,
          continueFrom: value.continueFrom || undefined,
          targetStage: value.targetStage || undefined,
          pipeline: value.pipeline || undefined,
          subtitleSource: value.subtitleSource || undefined,
          sourceLang: value.sourceLang || undefined,
          targetLang: value.targetLang || undefined,
        },
        servers: {
          ...((prev.servers ?? {}) as Record<string, any>),
          action: value.serverAction || undefined,
          name: value.serverName || undefined,
        },
      };
      try {
        await writeMut.mutateAsync([`${INPUT_PATH}.bak`, raw]);
        await writeMut.mutateAsync([INPUT_PATH, JSON.stringify(merged, null, 2)]);
        await qc.invalidateQueries({
          queryKey: client.read_app_file_text.queryKey(INPUT_PATH),
        });
        form.reset(value);
      } catch (e) {
        toastError(e as Error);
      }
    },
  }));

  createEffect(() => {
    const raw = fileQ.data;
    if (raw == null || filled) return;
    filled = true;
    const p = parseJsonc(raw);
    const t = (p.task ?? {}) as Record<string, any>;
    const s = (p.servers ?? {}) as Record<string, any>;
    form.reset({
      command: typeof p.command === "string" ? p.command : "task",
      action: String(t.action ?? ""),
      url: String(t.url ?? ""),
      taskDir: String(t.taskDir ?? ""),
      continueFrom: String(t.continueFrom ?? ""),
      targetStage: String(t.targetStage ?? ""),
      pipeline: String(t.pipeline ?? ""),
      subtitleSource: String(t.subtitleSource ?? ""),
      sourceLang: String(t.sourceLang ?? ""),
      targetLang: String(t.targetLang ?? ""),
      serverAction: String(s.action ?? ""),
      serverName: String(s.name ?? ""),
    });
  });

  return (
    <div class="space-y-4 flex flex-col h-full min-h-0">
      <div class="flex items-center gap-2">
        <h2>全局输入参数</h2>
        <form.Subscribe selector={(s) => ({ dirty: s.isDirty, submitting: s.isSubmitting })}>
          {(s) => (
            <Button
              size="sm"
              onClick={() => form.handleSubmit()}
              disabled={!s().dirty || s().submitting}
            >
              {s().submitting ? "保存中..." : s().dirty ? "保存" : "未修改"}
            </Button>
          )}
        </form.Subscribe>
      </div>
      <div class="flex-1 min-h-0">
        <Show when={fileQ.isLoading}>
          <div class="text-sm text-gray-500">加载中...</div>
        </Show>
        <Show when={fileQ.isSuccess}>
          <ScrollArea class="h-full">
            <h3>命令</h3>
            <form.Field name="command">
              {(field) => (
                <CardSelect
                  title="command"
                  description="顶层命令: task / servers / env / cookie"
                  field={field}
                  options={COMMANDS}
                />
              )}
            </form.Field>
            <form.Field name="action">
              {(field) => (
                <CardSelect
                  title="task.action"
                  description="任务动作 (start 导入并跑 / continue 续跑 / import 只导入 / enqueue_* 入队)"
                  field={field}
                  options={TASK_ACTIONS}
                />
              )}
            </form.Field>
            <h3>目标</h3>
            <form.Field name="url">
              {(field) => (
                <CardInput
                  title="task.url"
                  description="视频路径或远程/云端 url (start / enqueue_start 用)"
                  field={field}
                  placeholder="/home/aa/下载/大/1.mp4"
                />
              )}
            </form.Field>
            <form.Field name="taskDir">
              {(field) => (
                <CardInput
                  title="task.taskDir"
                  description="任务目录 (continue / enqueue_continue / status 用)"
                  field={field}
                  placeholder="workfolder/大/90"
                />
              )}
            </form.Field>
            <h3>续跑范围</h3>
            <form.Field name="continueFrom">
              {(field) => (
                <CardSelect
                  title="task.continueFrom"
                  description="从哪个 stage 开始续跑 (空 = 不指定)"
                  field={field}
                  options={STAGES}
                />
              )}
            </form.Field>
            <form.Field name="targetStage">
              {(field) => (
                <CardSelect
                  title="task.targetStage"
                  description="跑到此 stage 后停止 (空 = 跑到最后)"
                  field={field}
                  options={STAGES}
                />
              )}
            </form.Field>
            <h3>管线</h3>
            <form.Field name="pipeline">
              {(field) => (
                <CardSelect
                  title="task.pipeline"
                  description="dub = 配音, subtitle = 字幕"
                  field={field}
                  options={PIPELINES}
                />
              )}
            </form.Field>
            <form.Field name="subtitleSource">
              {(field) => (
                <CardSelect
                  title="task.subtitleSource"
                  description="字幕来源: asr = 语音识别(默认), sf_ocr = 关键帧 OCR, asr_ocr = 两者合并"
                  field={field}
                  options={SUBTITLE_SOURCES}
                />
              )}
            </form.Field>
            <form.Field name="sourceLang">
              {(field) => (
                <CardInput
                  title="task.sourceLang"
                  description="源语言 (空 = 自动)"
                  field={field}
                  placeholder="ja"
                />
              )}
            </form.Field>
            <form.Field name="targetLang">
              {(field) => (
                <CardInput
                  title="task.targetLang"
                  description="目标语言 (空 = 自动)"
                  field={field}
                  placeholder="zh"
                />
              )}
            </form.Field>
            <h3>服务器</h3>
            <form.Field name="serverAction">
              {(field) => (
                <CardSelect
                  title="servers.action"
                  description="servers 命令动作"
                  field={field}
                  options={SERVER_ACTIONS}
                />
              )}
            </form.Field>
            <form.Field name="serverName">
              {(field) => (
                <CardSelect
                  title="servers.name"
                  description="服务器类型: main = 主服务器, voxcpm_torch_gradio = 本地 TTS"
                  field={field}
                  options={SERVER_NAMES}
                />
              )}
            </form.Field>
            <p class="text-xs text-gray-500">
              保存会重写 {INPUT_PATH}（文件内的注释不保留），原文件自动备份到 {INPUT_PATH}.bak；
              未在此列出的字段（各 stage 详细参数）保持原值，需要在 input.jsonc 文本页编辑。
            </p>
          </ScrollArea>
        </Show>
      </div>
    </div>
  );
}
