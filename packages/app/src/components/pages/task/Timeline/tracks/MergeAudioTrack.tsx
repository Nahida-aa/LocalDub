import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import type { Track, TrackSegment } from "../consts";
import { TrackEditModal } from "./comp/TrackEditModal";
import { client } from "#/integrations/fnrpc/client.ts";
import type { Timing, TimingsFile } from "@repo/core/stages/merge_audio/types";
import { useTrackData } from "./useTrackData";
import type { BaseTrackProps } from "./shared";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[]): string {
  const segs: Timing[] = segments.map((s) => {
    const raw = (s.raw as Partial<Timing> | undefined) ?? {};
    return {
      seg_idx: raw.seg_idx ?? s.index + 1,
      src: raw.src ?? "",
      dst: s.text,
      src_lang: raw.src_lang ?? "auto",
      dst_lang: raw.dst_lang ?? "vi",
      start: raw.start ?? s.startMs,
      end: raw.end ?? s.endMs,
      speaker: raw.speaker ?? "1",
      original_duration_ms: raw.original_duration_ms ?? s.endMs - s.startMs,
      tts_duration_ms: raw.tts_duration_ms ?? 0,
      stretched_duration_ms: raw.stretched_duration_ms ?? 0,
      stretch_ratio: raw.stretch_ratio ?? 1,
      drift_ms: raw.drift_ms ?? 0,
      advance_ms: raw.advance_ms ?? 0,
      delay_ms: raw.delay_ms ?? 0,
      actual_start: s.startMs,
      actual_end: s.endMs,
    };
  });
  return JSON.stringify({ translation: segs }, null, 2);
}

const DEFAULT_DURATION_MS = 500;

function newDefaultRaw(startMs: number, endMs: number): Timing {
  return {
    seg_idx: 0,
    src: "",
    dst: "",
    src_lang: "auto",
    dst_lang: "vi",
    start: startMs,
    end: endMs,
    speaker: "1",
    original_duration_ms: endMs - startMs,
    tts_duration_ms: 0,
    stretched_duration_ms: 0,
    stretch_ratio: 1,
    drift_ms: 0,
    advance_ms: 0,
    delay_ms: 0,
    actual_start: startMs,
    actual_end: endMs,
  };
}

function insertAt(segments: TrackSegment[], index: number, after: boolean): TrackSegment[] {
  const copy = [...segments];
  const current = copy[index];
  if (!current) return copy;

  const startMs = after ? current.endMs : Math.max(0, current.startMs - DEFAULT_DURATION_MS);
  const endMs = after ? current.endMs + DEFAULT_DURATION_MS : current.startMs;
  const newSeg: TrackSegment = {
    index: -1,
    text: "",
    startMs,
    endMs,
    raw: newDefaultRaw(startMs, endMs),
  };

  if (after) {
    const next = copy[index + 1];
    if (next) {
      const gap = next.startMs - current.endMs;
      newSeg.endMs = current.endMs + Math.min(gap / 2, DEFAULT_DURATION_MS);
      newSeg.raw = newDefaultRaw(newSeg.startMs, newSeg.endMs);
    }
    copy.splice(index + 1, 0, newSeg);
  } else {
    const prev = copy[index - 1];
    if (prev) {
      const gap = current.startMs - prev.endMs;
      newSeg.startMs = current.startMs - Math.min(gap / 2, DEFAULT_DURATION_MS);
      newSeg.raw = newDefaultRaw(newSeg.startMs, newSeg.endMs);
    }
    copy.splice(index, 0, newSeg);
  }

  return copy.map((s, i) => ({ ...s, index: i }));
}

function deleteAt(segments: TrackSegment[], index: number): TrackSegment[] {
  return segments.filter((_, i) => i !== index).map((s, i) => ({ ...s, index: i }));
}

export function MergeAudioTrack(props: Props) {
  const { taskDir, pxPerMs, onSeek, color } = props;
  const track = () => props.track;
  const { segments } = useTrackData({
    taskDir,
    trackId: track().id,
    path: () => `${taskDir}/merge_audio/timings.json`,
    parse: (text) => {
      const data: TimingsFile = JSON.parse(text);
      return (data.translation || []).map((item, i: number) => ({
        index: i,
        text: item.dst,
        startMs: item.actual_start,
        endMs: item.actual_end,
        raw: item,
      }));
    },
    label: () => "merge_audio/timings.json",
  });
  const filePath = () => `${taskDir}/merge_audio/timings.json`;

  const handleInsertBefore = async (segIndex: number) => {
    const newSegments = insertAt(segments(), segIndex, false);
    await client.write_app_file_text.call([filePath(), serializeSegments(newSegments)]);
  };

  const handleInsertAfter = async (segIndex: number) => {
    const newSegments = insertAt(segments(), segIndex, true);
    await client.write_app_file_text.call([filePath(), serializeSegments(newSegments)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = segments()[segIndex];
    if (!seg) return;
    const raw = seg.raw as Timing | undefined;

    openModal(
      () => (
        <TrackEditModal
          textLabel="译文"
          srcLabel="原文"
          initialSrc={raw?.src}
          initialText={seg.text}
          initialStartMs={seg.startMs}
          initialEndMs={seg.endMs}
          extraFields={() =>
            raw && (
              <div class="flex gap-4 text-xs text-muted-foreground">
                <span>
                  语言: {raw.src_lang}→{raw.dst_lang}
                </span>
              </div>
            )
          }
          onSave={({ text, src, startMs, endMs }) => {
            const newSegments = segments().map((s, i) =>
              i === segIndex
                ? {
                    ...s,
                    text,
                    startMs,
                    endMs,
                    raw: { ...(s.raw as object), ...(src !== undefined ? { src } : {}) },
                  }
                : s,
            );
            return client.write_app_file_text.call([filePath(), serializeSegments(newSegments)]);
          }}
        />
      ),
      { title: "编辑合并片段" },
    );
  };

  const handleDelete = async (segIndex: number) => {
    const newSegments = deleteAt(segments(), segIndex);
    await client.write_app_file_text.call([filePath(), serializeSegments(newSegments)]);
  };

  const segs = segments();
  if (!segs.length) return null;

  return (
    <div class="h-16 border-b relative">
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
              title={(seg.raw as Timing | undefined)?.src || seg.text}
            >
              {(seg.raw as Timing | undefined)?.src || seg.text}
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
            <ContextMenuItem onSelect={() => handleDelete(seg.index)} class="text-destructive">
              删除
            </ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      ))}
    </div>
  );
}
