//! 全局输入参数表单: 以表单编辑仓库根 input.jsonc 的常用字段。
//!
//! 与 `input.jsonc` 文本 tab 并存: 表单适合快速改常用字段, 冷门/未覆盖字段
//! (各 stage 的详细参数) 仍走文本编辑。
//!
//! 保存语义: 用 jsonc-parser 的 modify/applyEdits 按字段路径定点编辑 —— 非空字段写入,
//! 空字段删除属性, **文件内注释与其他字段原样保留**; 写前仍备份到 `input.jsonc.bak`。
//!
//! 表单状态由 `useAppForm` (tanstack/solid-form) 管理。
//! 常量选项见 `./options`, 字段卡片组件见 `./FieldCards`。

import { createEffect, Show } from "solid-js";
import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { toastError } from "@repo/ui-solid/custom/toast";
import { client } from "#/integrations/fnrpc/client.ts";
import { useAppForm } from "@repo/ui-solid/form/useAppForm";
import { applyEdits, modify, parse } from "jsonc-parser";
import { ScrollArea } from "@repo/ui-solid/base/scroll-area";
import { CardInput, CardSelect } from "./FieldCards";
import {
  COMMANDS,
  LANGS,
  PIPELINES,
  SERVER_ACTIONS,
  SERVER_NAMES,
  STAGES,
  SUBTITLE_SOURCES,
  WORKFLOW_ACTIONS,
  langLabel,
} from "./options";

const INPUT_PATH = "input.jsonc";

type FormState = {
  command: string;
  action: string;
  url: string;
  workflowDir: string;
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
  command: "workflow",
  action: "",
  url: "",
  workflowDir: "",
  continueFrom: "",
  targetStage: "",
  pipeline: "",
  subtitleSource: "",
  sourceLang: "",
  targetLang: "",
  serverAction: "",
  serverName: "",
});

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
        set(["workflow", "action"], value.action);
        set(["workflow", "url"], value.url);
        set(["workflow", "workflowDir"], value.workflowDir);
        set(["workflow", "continueFrom"], value.continueFrom);
        set(["workflow", "targetStage"], value.targetStage);
        set(["workflow", "pipeline"], value.pipeline);
        set(["workflow", "subtitleSource"], value.subtitleSource);
        set(["workflow", "sourceLang"], value.sourceLang);
        set(["workflow", "targetLang"], value.targetLang);
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
    const t = (p.workflow ?? {}) as Record<string, any>;
    const s = (p.servers ?? {}) as Record<string, any>;
    form.reset({
      command: typeof p.command === "string" ? p.command : "workflow",
      action: String(t.action ?? ""),
      url: String(t.url ?? ""),
      workflowDir: String(t.workflowDir ?? ""),
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
        <h2>全局参数</h2>
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
                  description="顶层命令: workflow / servers / env / cookie"
                  field={field}
                  options={COMMANDS}
                />
              )}
            </form.Field>
            <form.Field name="action">
              {(field) => (
                <CardSelect
                  title="workflow.action"
                  description="workflow 动作 (start 导入并跑 / continue 续跑 / import 只导入 / enqueue_* 入队)"
                  field={field}
                  options={WORKFLOW_ACTIONS}
                />
              )}
            </form.Field>
            <h3>目标</h3>
            <form.Field name="url">
              {(field) => (
                <CardInput
                  title="workflow.url"
                  description="视频路径或远程/云端 url (start / enqueue_start 用)"
                  field={field}
                  placeholder="/home/aa/下载/大/1.mp4"
                />
              )}
            </form.Field>
            <form.Field name="workflowDir">
              {(field) => (
                <CardInput
                  title="workflow.workflowDir"
                  description="workflow 目录 (continue / enqueue_continue / status 用)"
                  field={field}
                  placeholder="workfolder/大/90"
                />
              )}
            </form.Field>
            <h3>续跑范围</h3>
            <form.Field name="continueFrom">
              {(field) => (
                <CardSelect
                  title="workflow.continueFrom"
                  description="从哪个 stage 开始续跑 (空 = 不指定)"
                  field={field}
                  options={STAGES}
                />
              )}
            </form.Field>
            <form.Field name="targetStage">
              {(field) => (
                <CardSelect
                  title="workflow.targetStage"
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
                  title="workflow.pipeline"
                  description="dub = 配音, subtitle = 字幕"
                  field={field}
                  options={PIPELINES}
                />
              )}
            </form.Field>
            <form.Field name="subtitleSource">
              {(field) => (
                <CardSelect
                  title="workflow.subtitleSource"
                  description="字幕来源: asr = 语音识别(默认), sf_ocr = 关键帧 OCR, asr_ocr = 两者合并"
                  field={field}
                  options={SUBTITLE_SOURCES}
                />
              )}
            </form.Field>
            <form.Field name="sourceLang">
              {(field) => (
                <CardSelect
                  title="workflow.sourceLang"
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
                  title="workflow.targetLang"
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
