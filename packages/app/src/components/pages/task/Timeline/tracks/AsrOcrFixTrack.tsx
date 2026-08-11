import { createSignal } from "solid-js";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal, closeModal } from "@repo/ui-solid/custom/modal/renderer";
import type { Track, TrackSegment } from "../consts";
import { client } from "#/integrations/fnrpc/client.ts";
import { useMutation } from "@tanstack/solid-query";
import { useViewingTab } from "../../TaskControlPanel/taskControlPanelStore";
import { STAGE_TRACKS } from "./const";
import { AsrOcrFile, OcrSegment } from "@repo/subtitle-ocr/types";
import { OcrSegmentFilterResult } from "@repo/subtitle-ocr/ocr_fix/segment_filter";
import { useTrackData } from "./useTrackData";
import type { BaseTrackProps } from "./shared";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[], trackId: string): string {
  const segs: OcrSegment[] = segments.map((s) => {
    const raw = (s.raw as OcrSegment) || {};
    return {
      text: s.text,
      start_ms: s.startMs,
      end_ms: s.endMs,
      y_range: raw.y_range ?? [0, 0],
      text_confidence: raw.text_confidence ?? 1,
    };
  });
  const merged = segs
    .map((s) => s.text)
    .filter(Boolean)
    .join(" ");
  // sf_ocr_fix 落盘 segment_filter_llm_fix/segment_filter 形状 (OcrSegmentFilterResult)，其余走 AsrOcrFile
  const out =
    trackId === "sf_ocr_fix"
      ? { result: { text: merged, segments: segs } }
      : { result: { segments: segs } };
  return JSON.stringify(out, null, 2);
}

const DEFAULT_DURATION_MS = 500;

function insertAt(segments: TrackSegment[], index: number, after: boolean): TrackSegment[] {
  const copy = [...segments];
  const current = copy[index];
  if (!current) return copy;

  const newSeg: TrackSegment = {
    index: -1,
    text: "",
    startMs: after ? current.endMs : Math.max(0, current.startMs - DEFAULT_DURATION_MS),
    endMs: after ? current.endMs + DEFAULT_DURATION_MS : current.startMs,
    raw: { text: "", start: 0, end: 0, box_y: [0, 0], confidence: 1 },
  };

  if (after) {
    // Check gap to next segment
    const next = copy[index + 1];
    if (next) {
      const gap = next.startMs - current.endMs;
      newSeg.endMs = current.endMs + Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index + 1, 0, newSeg);
  } else {
    // Check gap to previous segment
    const prev = copy[index - 1];
    if (prev) {
      const gap = current.startMs - prev.endMs;
      newSeg.startMs = current.startMs - Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index, 0, newSeg);
  }

  return copy.map((s, i) => ({ ...s, index: i }));
}

function deleteAt(segments: TrackSegment[], index: number): TrackSegment[] {
  return segments.filter((_, i) => i !== index).map((s, i) => ({ ...s, index: i }));
}

export function AsrOcrFixTrack(props: Props) {
  const { taskDir, pxPerMs, onSeek, color } = props;
  const track = () => props.track;
  const isSf = () => track().id === "sf_ocr_fix";
  const viewingTab = useViewingTab();

  const primaryPath = () =>
    isSf()
      ? `${taskDir}/sf_ocr_fix/segment_filter_llm_fix.json`
      : `${taskDir}/asr_ocr_fix/asr_ocr_fused_llm_fix.json`;
  const fallbackPath = () => `${taskDir}/sf_ocr_fix/segment_filter.json`;

  // sf_ocr_fix：优先 LLM 修正产物，读取失败则回落到段过滤产物
  const { q, fb, active, segments } = useTrackData({
    taskDir,
    trackId: track().id,
    path: primaryPath,
    fallbackPath: () => (isSf() ? fallbackPath() : undefined),
    parse: (text) => {
      if (isSf()) {
        const data = JSON.parse(text) as OcrSegmentFilterResult;
        return (data.result?.segments ?? []).map((item, i: number) => ({
          index: i,
          text: item.text,
          startMs: item.start_ms,
          endMs: item.end_ms,
          raw: item,
        }));
      }
      const data = JSON.parse(text) as AsrOcrFile;
      return data.result.segments.map((item, i: number) => ({
        index: i,
        text: item.text,
        startMs: item.start_ms,
        endMs: item.end_ms,
        raw: item,
      }));
    },
    label: () =>
      isSf()
        ? q.isSuccess
          ? "sf_ocr_fix/segment_filter_llm_fix.json"
          : "sf_ocr_fix/segment_filter.json"
        : "asr_ocr_fix/asr_ocr_fused_llm_fix.json",
  });
  const filePath = () => (active() === fb ? fallbackPath() : primaryPath());

  // ---- 联动删除（校对 + 译文同步删同索引）：占位，待服务端 RPC，见 ROADMAP.md ----
  const tabTracks = () => {
    const v = viewingTab();
    return v === "root" ? [] : (STAGE_TRACKS[v] ?? []);
  };
  const showLinkedDelete = () =>
    tabTracks().includes("asr_ocr_fix") && tabTracks().includes("translation");

  const mutation = useMutation(() =>
    client.write_app_file_text.mutationOptions({
      onMutate: (variables, context) => {
        context.client.setQueryData(client.read_app_file_text.queryKey(variables[0]), variables[1]);
      },
      onSuccess: (data, variables, onMutateResult, context) => {
        context.client.invalidateQueries({
          queryKey: client.read_app_file_text.queryKey(variables[0]),
        });
      },
    }),
  );
  const handleInsertBefore = (segIndex: number) => {
    const newSegments = insertAt(segments(), segIndex, false);
    mutation.mutate([filePath(), serializeSegments(newSegments, track().id)]);
  };

  const handleInsertAfter = (segIndex: number) => {
    const newSegments = insertAt(segments(), segIndex, true);
    mutation.mutate([filePath(), serializeSegments(newSegments, track().id)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = segments()[segIndex];
    if (!seg) return;
    const raw = seg.raw as OcrSegment | undefined;

    openModal(
      () => {
        const [text, setText] = createSignal(seg.text);
        const [startMs, setStartMs] = createSignal(seg.startMs);
        const [endMs, setEndMs] = createSignal(seg.endMs);

        const onSave = () => {
          const newSegments = segments().map((s, i) =>
            i === segIndex ? { ...s, text: text(), startMs: startMs(), endMs: endMs() } : s,
          );
          mutation.mutate([filePath(), serializeSegments(newSegments, track().id)]);
          closeModal();
        };

        return (
          <div class="flex flex-col gap-3 p-2 text-sm">
            <label class="flex flex-col gap-1">
              <span class="font-medium">文本</span>
              <textarea
                class="w-full min-h-20 rounded border p-2 text-sm"
                value={text()}
                onInput={(e) => setText(e.currentTarget.value)}
              />
            </label>
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
            {raw && (
              <div class="flex gap-4 text-xs text-muted-foreground">
                <span>置信度: {raw.text_confidence?.toFixed(3)}</span>
                <span>
                  y_range: [{raw.y_range?.[0]}, {raw.y_range?.[1]}]
                </span>
              </div>
            )}
            <div class="flex justify-end gap-2 mt-1">
              <button
                class="px-3 py-1.5 rounded border text-sm cursor-pointer"
                onClick={closeModal}
              >
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
      },
      { title: "编辑片段" },
    );
  };

  const handleDelete = (segIndex: number) => {
    const newSegments = deleteAt(segments(), segIndex);
    mutation.mutate([filePath(), serializeSegments(newSegments, track().id)]);
  };

  const segs = segments();
  if (!segs.length) return null;

  return (
    <div class="h-16 border-b relative" data-vtab={viewingTab()}>
      {segs.map((seg) => (
        <ContextMenu>
          <ContextMenuTrigger as="div" class="contents">
            <div
              class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
              style={{
                left: `${seg.startMs * pxPerMs}px`,
                width: `${Math.max((seg.endMs - seg.startMs) * pxPerMs, 4)}px`,
                background: `${color}33`,
                "border-color": `${color}55`,
              }}
              onClick={() => onSeek(seg.startMs)}
              title={seg.text}
            >
              {seg.text}
            </div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem onSelect={() => handleInsertBefore(seg.index)}>
              向前插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleInsertAfter(seg.index)}>
              向后插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleEdit(seg.index)}>编辑</ContextMenuItem>
            <ContextMenuItem onSelect={() => onSeek(seg.endMs)}>跳转到结尾</ContextMenuItem>
            <ContextMenuSeparator />
            {showLinkedDelete() && (
              <ContextMenuItem
                onSelect={() => console.log("[LINKED-DELETE] 开发中，待服务端 RPC，见 ROADMAP.md")}
                class="text-destructive"
              >
                联动删除(校对+译文) · 开发中
              </ContextMenuItem>
            )}
            <ContextMenuItem onSelect={() => handleDelete(seg.index)} class="text-destructive">
              删除
            </ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      ))}
    </div>
  );
}
