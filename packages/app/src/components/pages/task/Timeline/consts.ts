import type { FrameRate } from "@repo/util/timecode";

export interface TrackSegment {
  index: number;
  text: string;
  startMs: number;
  endMs: number;
  raw?: unknown;
}

export interface Track {
  id: string;
  label: string;
  segments: TrackSegment[];
  color?: string;
  filePath?: string;
}

export const BASE_PX_PER_MS = 0.05;
export const DEFAULT_COLORS = ["#3b82f6", "#22c55e", "#a855f7", "#f59e0b", "#ef4444", "#ec4899"];

const LABEL_FRAME_INTERVALS = [2, 3, 5, 10, 15];
const TICK_FRAME_INTERVALS = [1, 2, 3, 5, 10, 15];
const SECOND_MULTIPLIERS = [1, 2, 3, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];
const MIN_LABEL_SPACING_PX = 120;
const MIN_TICK_SPACING_PX = 18;

export interface RulerConfig {
  labelIntervalMs: number;
  tickIntervalMs: number;
}

function computeIntervalMs(
  pxPerFrame: number,
  pxPerSec: number,
  fps: number,
  minPx: number,
  frameIntervals: number[],
): number {
  for (const fi of frameIntervals) {
    if (pxPerFrame * fi >= minPx) return (fi / fps) * 1000;
  }
  for (const sm of SECOND_MULTIPLIERS) {
    if (pxPerSec * sm >= minPx) return sm * 1000;
  }
  return 60 * 1000;
}

function alignTickToLabel(labelMs: number, tickMs: number, fps: number): number {
  if (tickMs >= labelMs) return labelMs;

  const frameMs = 1000 / fps;
  const labelFrames = Math.round(labelMs / frameMs);

  if (Math.abs(labelFrames * frameMs - labelMs) < 0.5) {
    let bestFrames = labelFrames;
    let bestDiff = Infinity;
    for (let f = 1; f <= labelFrames; f++) {
      if (labelFrames % f === 0) {
        const diff = Math.abs(f - Math.round(tickMs / frameMs));
        if (diff < bestDiff || (diff === bestDiff && f > bestFrames)) {
          bestDiff = diff;
          bestFrames = f;
        }
      }
    }
    return (bestFrames / fps) * 1000;
  }

  const labelSec = labelMs / 1000;
  const candidates = SECOND_MULTIPLIERS.filter((sm) => sm < labelSec && labelSec % sm === 0);
  if (candidates.length > 0) {
    const tickSec = tickMs / 1000;
    const best = candidates.reduce((a, b) =>
      Math.abs(a - tickSec) <= Math.abs(b - tickSec) ? a : b,
    );
    return best * 1000;
  }

  return tickMs;
}

export function rulerConfig(pxPerMs: number, fps: FrameRate): RulerConfig {
  const fpsFloat = fps.numerator / fps.denominator;
  const pxPerSec = pxPerMs * 1000;
  const pxPerFrame = pxPerSec / fpsFloat;

  const labelMs = computeIntervalMs(
    pxPerFrame,
    pxPerSec,
    fpsFloat,
    MIN_LABEL_SPACING_PX,
    LABEL_FRAME_INTERVALS,
  );
  const rawTickMs = computeIntervalMs(
    pxPerFrame,
    pxPerSec,
    fpsFloat,
    MIN_TICK_SPACING_PX,
    TICK_FRAME_INTERVALS,
  );
  const tickMs = alignTickToLabel(labelMs, rawTickMs, fpsFloat);

  return { labelIntervalMs: labelMs, tickIntervalMs: tickMs };
}

export function shouldShowLabel(ms: number, labelIntervalMs: number): boolean {
  if (labelIntervalMs <= 0) return false;
  const ratio = ms / labelIntervalMs;
  return Math.abs(ratio - Math.round(ratio)) < 0.001;
}

export function msToRuler(ms: number, fps?: number) {
  const isSecondBoundary = Math.abs(ms % 1000) < 0.5;
  if (fps !== undefined && !isSecondBoundary) {
    const frameMs = 1000 / fps;
    const frame = Math.round((ms % 1000) / frameMs);
    return `${frame}f`;
  }
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function trackColor(index: number, track: Track) {
  return track.color ?? DEFAULT_COLORS[index % DEFAULT_COLORS.length];
}
