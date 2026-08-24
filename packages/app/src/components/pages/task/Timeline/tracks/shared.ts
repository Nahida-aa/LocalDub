import type { Track, TrackSegment } from "../consts";

/** 各轨道组件公共 props（数据由轨道组件内部自取，父层只下发 id/label/color 描述符） */
export interface BaseTrackProps {
  track: Track;
  totalPx: number;
  pxPerMs: number;
  onSeek: (ms: number) => void;
  color: string;
  taskDir: string;
}

export function deleteAt(segments: TrackSegment[], index: number): TrackSegment[] {
  return segments.filter((_, i) => i !== index).map((s, i) => ({ ...s, index: i }));
}

const DEFAULT_DURATION_MS = 500;

export interface InsertAtOptions {
  /** 新片段的默认文本，默认空串 */
  defaultText?: string;
  /** 新片段的默认 raw（不同轨道格式不同） */
  defaultRaw?: unknown;
}

export function insertAt(
  segments: TrackSegment[],
  index: number,
  after: boolean,
  opts: InsertAtOptions = {},
): TrackSegment[] {
  const copy = [...segments];
  const current = copy[index];
  if (!current) return copy;

  const newSeg: TrackSegment = {
    index: -1,
    text: opts.defaultText ?? "",
    startMs: after ? current.endMs : Math.max(0, current.startMs - DEFAULT_DURATION_MS),
    endMs: after ? current.endMs + DEFAULT_DURATION_MS : current.startMs,
    raw: opts.defaultRaw,
  };

  if (after) {
    const next = copy[index + 1];
    if (next) {
      const gap = next.startMs - current.endMs;
      newSeg.endMs = current.endMs + Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index + 1, 0, newSeg);
  } else {
    const prev = copy[index - 1];
    if (prev) {
      const gap = current.startMs - prev.endMs;
      newSeg.startMs = current.startMs - Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index, 0, newSeg);
  }

  return copy.map((s, i) => ({ ...s, index: i }));
}
