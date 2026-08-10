import {
  FrameResult,
  OcrBoxResult,
  OcrSegment,
  OcrSegmentWithAdjust,
} from "@repo/subtitle-ocr/types";
import { AsrOcrFixArgs, BoxAdjustedArgs, MergeFramesArgs } from "@repo/subtitle-ocr/args";
import { OcrSegmentAdjustArgs } from "@repo/subtitle-ocr/args";
import { aggregate_boxes } from "@repo/subtitle-ocr/ocr_util";
import { YStats } from "@repo/subtitle-ocr/ocr_fix/stats";

export function ocr_segment_adjust(
  segments: OcrSegment[],
  frameResults: FrameResult[],
  yStats: YStats,
  videoHeight: number,
  { isoThresholdMs, adjustYWeight, adjustIsoWeight, adjustYFactor }: OcrSegmentAdjustArgs,
): OcrSegmentWithAdjust[] {
  if (segments.length === 0 || (!yStats.avg[0] && yStats.avg[1] === 0)) return segments;

  const avgCentroid = (yStats.avg[0] + yStats.avg[1]) / 2;

  // build sorted non-empty frame timestamps for isolation search
  const nonEmptyTs = frameResults
    .filter((f) => f.text && f.x_range && f.y_range)
    .map((f) => f.timestamp)
    .sort((a, b) => a - b);

  return segments.map((seg) => {
    if (seg.frame_count === undefined || seg.text_confidence === undefined) return seg;

    // Y penalty: centroid offset relative to video height
    let yPenalty = 0;
    if (seg.y_range) {
      const centroid = (seg.y_range[0] + seg.y_range[1]) / 2;
      const offset = Math.abs(centroid - avgCentroid);
      yPenalty = Math.min(1, offset / (videoHeight * adjustYFactor));
    }

    // Isolation penalty: only for single-frame segments
    let isoPenalty = 0;
    if (seg.frame_count === 1) {
      const mid = (seg.start_ms + seg.end_ms) / 2;
      const nonEmptyBefore = [...nonEmptyTs].reverse().find((t) => t < mid);
      const nonEmptyAfter = nonEmptyTs.find((t) => t > mid);
      const gapBefore = nonEmptyBefore !== undefined ? mid - nonEmptyBefore : Infinity;
      const gapAfter = nonEmptyAfter !== undefined ? nonEmptyAfter - mid : Infinity;
      const nearestGap = Math.min(gapBefore, gapAfter);
      isoPenalty = Math.min(1, nearestGap / isoThresholdMs);
    }

    const totalPenalty = adjustYWeight * yPenalty + adjustIsoWeight * isoPenalty;
    const adjustedConfidence = seg.text_confidence * Math.max(0, 1 - totalPenalty);

    return {
      ...seg,
      adjusted_confidence: Math.round(adjustedConfidence * 100) / 100,
      y_penalty: Math.round(yPenalty * 100) / 100,
      iso_penalty: Math.round(isoPenalty * 100) / 100,
    };
  });
}
