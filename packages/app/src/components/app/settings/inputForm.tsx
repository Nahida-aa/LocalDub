//! 全局输入参数表单: 以表单编辑仓库根 input.jsonc 的常用字段。
//!
//! 与 `input.jsonc` 文本 tab 并存: 表单适合快速改常用字段, 冷门/未覆盖字段
//! (各 stage 的详细参数) 仍走文本编辑。
//!
//! 保存语义: 用 jsonc-parser 的 modify/applyEdits 按字段路径定点编辑 —— 非空字段写入,
//! 空字段删除属性, **文件内注释与其他字段原样保留**; 写前仍备份到 `input.jsonc.bak`。
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
import { applyEdits, modify, parse } from "jsonc-parser";

const INPUT_PATH = "input.jsonc";

import type {
  Pipeline,
  ServerAction,
  ServerType_Serialize,
  StageName,
  SubtitleSource,
  TargetLang,
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

// 语言码: 与 Rust `const::lang::LANGS` (bindings TargetLang) 同步。
// sourceLang 值域开放 (whisper ~99 种), 这里只列可翻译的 23 种已知语言,
// 列表外语言码 (it/uk/nl...) 需在 input.jsonc 文本页直接填。
const LANGS = keysOf({
  en: "",
  zh: "",
  vi: "",
  ja: "",
  ko: "",
  fr: "",
  de: "",
  es: "",
  pt: "",
  ru: "",
  ar: "",
  hi: "",
  th: "",
  id: "",
  ms: "",
  tl: "",
  my: "",
  km: "",
  lo: "",
  mn: "",
  ne: "",
  ur: "",
  bn: "",
} satisfies Record<TargetLang, string>);

const LANG_LABELS: Record<string, string> = {
  en: "英语 English",
  zh: "中文 Chinese",
  vi: "越南语 Vietnamese",
  ja: "日语 Japanese",
  ko: "韩语 Korean",
  fr: "法语 French",
  de: "德语 German",
  es: "西班牙语 Spanish",
  pt: "葡萄牙语 Portuguese",
  ru: "俄语 Russian",
  ar: "阿拉伯语 Arabic",
  hi: "印地语 Hindi",
  th: "泰语 Thai",
  id: "印尼语 Indonesian",
  ms: "马来语 Malay",
  tl: "他加禄语 Tagalog",
  my: "缅甸语 Burmese",
  km: "高棉语 Khmer",
  lo: "老挝语 Lao",
  mn: "蒙古语 Mongolian",
  ne: "尼泊尔语 Nepali",
  ur: "乌尔都语 Urdu",
  bn: "孟加拉语 Bengali",
};

const langLabel = (v: string): string => (LANG_LABELS[v] ? `${v} · ${LANG_LABELS[v]}` : v);

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
  optionLabel?: (v: string) => string;
}) {
  // Solid: JSX 属性需是"调用"才会编译成 getter —— 直接写对象字面量会被静态化,
  // signal 变化时选中项不同步 (React 靠重渲染天然正确, Solid 不行)。
  const labelOf = (v: string): string => (props.optionLabel ? props.optionLabel(v) : v) || "—";
  const selected = (): Option => ({
    value: props.field().state.value || EMPTY,
    label: labelOf(props.field().state.value || ""),
  });
  const options = (): Option[] =>
    props.options.map((v) => ({ value: v || EMPTY, label: labelOf(v) }));

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
          <SelectTrigger class="w-54">
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
      // jsonc-parser modify 按路径定点编辑: 空字段传 undefined 删除属性,
      // 文件内注释 + 未列出的字段不动。
      const fmt = { tabSize: 2, insertSpaces: true } as const;
      let next = raw;
      const set = (path: (string | number)[], v: string) => {
        next = applyEdits(next, modify(next, path, v || undefined, { formattingOptions: fmt }));
      };
      try {
        await writeMut.mutateAsync([`${INPUT_PATH}.bak`, raw]);
        set(["command"], value.command);
        set(["task", "action"], value.action);
        set(["task", "url"], value.url);
        set(["task", "taskDir"], value.taskDir);
        set(["task", "continueFrom"], value.continueFrom);
        set(["task", "targetStage"], value.targetStage);
        set(["task", "pipeline"], value.pipeline);
        set(["task", "subtitleSource"], value.subtitleSource);
        set(["task", "sourceLang"], value.sourceLang);
        set(["task", "targetLang"], value.targetLang);
        set(["servers", "action"], value.serverAction);
        set(["servers", "name"], value.serverName);
        await writeMut.mutateAsync([INPUT_PATH, next]);
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
    const p = parse(raw) as Record<string, any>;
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
                <CardSelect
                  title="task.sourceLang"
                  description="源语言 (空 = 自动; 列表外如 it/uk 需在 input.jsonc 文本页填)"
                  field={field}
                  options={LANGS}
                  optionLabel={langLabel}
                />
              )}
            </form.Field>
            <form.Field name="targetLang">
              {(field) => (
                <CardSelect
                  title="task.targetLang"
                  description="目标语言 (空 = 自动)"
                  field={field}
                  options={LANGS}
                  optionLabel={langLabel}
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
              保存只更新表单覆盖的字段（文件内注释与未列出的字段保持不变），写前仍备份到{" "}
              {INPUT_PATH}.bak；各 stage 详细参数需要在 input.jsonc 文本页编辑。
            </p>
          </ScrollArea>
        </Show>
      </div>
    </div>
  );
}
