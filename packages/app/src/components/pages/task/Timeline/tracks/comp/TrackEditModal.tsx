import { createSignal, Show, type JSX } from "solid-js";
import { closeModal } from "@repo/ui-solid/custom/modal/renderer";

interface TrackEditModalProps {
  /** 文本输入框的 label，默认 "文本"（如译文轨道用 "译文"） */
  textLabel?: string;
  initialText: string;
  /** 提供则渲染可编辑的"原文"输入框（src），默认不渲染 */
  initialSrc?: string;
  srcLabel?: string;
  initialStartMs: number;
  initialEndMs: number;
  /** 额外字段展示（如置信度/box_y、语言），插槽自由扩展 */
  extraFields?: () => JSX.Element;
  /** 保存回调，负责写盘；成功返回后自动关闭弹窗 */
  onSave: (values: {
    text: string;
    src?: string;
    startMs: number;
    endMs: number;
  }) => Promise<void> | void;
}

/** 通用轨道编辑弹窗：文本 + 可选原文 + 开始/结束 + 可插拔额外字段 */
export function TrackEditModal(props: TrackEditModalProps) {
  const [text, setText] = createSignal(props.initialText);
  const [src, setSrc] = createSignal(props.initialSrc ?? "");
  const [startMs, setStartMs] = createSignal(props.initialStartMs);
  const [endMs, setEndMs] = createSignal(props.initialEndMs);

  const hasSrc = () => props.initialSrc !== undefined;

  const onSave = async () => {
    await props.onSave({
      text: text(),
      src: hasSrc() ? src() : undefined,
      startMs: startMs(),
      endMs: endMs(),
    });
    closeModal();
  };

  return (
    <div class="flex flex-col gap-3 p-2 text-sm">
      <label class="flex flex-col gap-1">
        <span class="font-medium">{props.textLabel ?? "文本"}</span>
        <textarea
          class="w-full min-h-20 rounded border p-2 text-sm"
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
        />
      </label>
      <Show when={hasSrc()}>
        <label class="flex flex-col gap-1">
          <span class="font-medium">{props.srcLabel ?? "原文"}</span>
          <textarea
            class="w-full min-h-12 rounded border p-2 text-sm"
            value={src()}
            onInput={(e) => setSrc(e.currentTarget.value)}
          />
        </label>
      </Show>
      <div class="flex gap-4">
        <label class="flex flex-col gap-1 flex-1">
          <span class="font-medium">开始 (ms)</span>
          <input
            class="rounded border px-2 py-1 text-sm"
            type="number"
            value={startMs()}
            onInput={(e) => setStartMs(Number(e.currentTarget.value))}
          />
        </label>
        <label class="flex flex-col gap-1 flex-1">
          <span class="font-medium">结束 (ms)</span>
          <input
            class="rounded border px-2 py-1 text-sm"
            type="number"
            value={endMs()}
            onInput={(e) => setEndMs(Number(e.currentTarget.value))}
          />
        </label>
      </div>
      {props.extraFields?.()}
      <div class="flex justify-end gap-2 mt-1">
        <button class="px-3 py-1.5 rounded border text-sm cursor-pointer" onClick={closeModal}>
          取消
        </button>
        <button
          class="px-3 py-1.5 rounded bg-primary text-primary-foreground text-sm cursor-pointer"
          onClick={onSave}
        >
          保存
        </button>
      </div>
    </div>
  );
}
