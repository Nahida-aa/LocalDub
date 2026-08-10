import {
  FrameResult,
  OcrSegment,
  OcrSegmentWithAdjust,
  SegmentFrame,
} from "@repo/subtitle-ocr/types";

/**
 * 从 ocr.json 的 segments 出发，按 segment confidence 过滤，生成 ocr_filtered.json 的结果。
 * 如果 segment 已带有 adjustedConfidence（Y 偏移 + 孤立惩罚后的置信度），则优先用它过滤。
 *
 * @param segments 来自 ocr.json.result.segments（mergeFrames 结果），或 computeSegmentAdjustments 的输出
 * @param textScore confidence 阈值，低于此值的 segment 会被丢弃。0 表示不过滤。
 * @returns 过滤后的 segments
 */
export function ocrSegmentFilter(
  segments: (OcrSegment | OcrSegmentWithAdjust)[],
  text_confidence_threshold: number,
): (OcrSegment | OcrSegmentWithAdjust)[] {
  if (!text_confidence_threshold || text_confidence_threshold <= 0) {
    return segments.map((s) => ({ ...s }));
  }
  const filtered = segments.filter((s) => {
    const text_confidence =
      "adjusted_text_confidence" in s ? s.adjusted_text_confidence : s.text_confidence;
    return text_confidence === undefined || text_confidence >= text_confidence_threshold;
  });
  return filtered;
}
type OcrSegmentFilterResult = {
  meta: {
    segment_count: number;
    text_confidence_threshold: number;
    dropped: number;
  };
  result: {
    text: string;
    segments: (OcrSegment | OcrSegmentWithAdjust)[];
  };
};
export function ocrSegmentFilterWithMeta(
  segments: (OcrSegment | OcrSegmentWithAdjust)[],
  text_confidence_threshold: number,
): OcrSegmentFilterResult {
  const filtered = ocrSegmentFilter(segments, text_confidence_threshold);
  return {
    meta: {
      segment_count: filtered.length,
      text_confidence_threshold,
      dropped: segments.length - filtered.length,
    },
    result: {
      text: filtered.map((s) => s.text).join(" "),
      segments: filtered,
    },
  };
}
