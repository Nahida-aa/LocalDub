import { FrameResult, OcrBoxResult } from "../types";
import { YStats, computeBoxYStats } from "./stats";
import { AsrOcrFixArgs, BoxAdjustedArgs, MergeFramesArgs } from "@repo/subtitle-ocr/args";
import { aggregate_boxes } from "../ocr_util";

type OcrBoxResultWithAdjust = OcrBoxResult & {
  top_offset_ratio: number;
  bot_offset_ratio: number;
  height: number;
  height_ratio: number;
  is_outlier: boolean;
  adjusted_text_confidence: number;
};
type FrameResultBoxWithAdjust = Omit<FrameResult, "boxes"> & {
  boxes: OcrBoxResultWithAdjust[];
};

type OcrFramesBoxAdjustResult = {
  frames: FrameResultBoxWithAdjust[];
  meta: OcrFramesBoxAdjustResultMeta;
};
type OcrFramesBoxAdjustResultMeta = {
  y_stats: YStats;
  frame_count: number;
  args: BoxAdjustedArgs;
};

export const build_ocr_frames_box_adjust = (
  frames: FrameResult[],
  yStats: YStats,
  args: BoxAdjustedArgs,
): OcrFramesBoxAdjustResult => {
  const annotatedFrames = frames.map((f) => ({
    ...f,
    boxes: f.boxes.map((box_r) => {
      if (!box_r.text.trim())
        return {
          ...box_r,
          top_offset_ratio: 0,
          bot_offset_ratio: 0,
          height: 0,
          height_ratio: 0,
          is_outlier: false,
          adjusted_text_confidence: box_r.text_confidence,
        };
      const top = box_r.y_range[0];
      const bottom = box_r.y_range[1];
      const height = bottom - top;
      // 这一行上边界，相对典型上边界，偏离了多少个行高
      const topOR =
        yStats.medianHeight > 0 ? Math.abs(top - yStats.mode[0]) / yStats.medianHeight : 0;
      const botOR =
        yStats.medianHeight > 0 ? Math.abs(bottom - yStats.mode[1]) / yStats.medianHeight : 0;
      const heightRatio =
        yStats.medianHeight > 0 ? Math.round((height / yStats.medianHeight) * 100) / 100 : 0;
      const bandDrift = Math.max(topOR, botOR); // 上下边界偏离取大的
      const noisePenalty = Math.min(
        1,
        Math.max(0, (bandDrift - 1.0) * 0.5) + // band 偏离 >1 行高才罚
          Math.abs(1 - heightRatio) * 0.3,
      );
      const adjustedConfidence = Math.round(box_r.text_confidence * (1 - noisePenalty) * 100) / 100;
      const isOutlier = adjustedConfidence < args.boxAdjustedThreshold;
      return {
        ...box_r,
        top_offset_ratio: Math.round(topOR * 100) / 100,
        bot_offset_ratio: Math.round(botOR * 100) / 100,
        height: Math.round(height * 10) / 10,
        height_ratio: heightRatio,
        is_outlier: isOutlier,
        adjusted_text_confidence: adjustedConfidence,
      };
    }),
  }));
  return {
    frames: annotatedFrames,
    meta: {
      y_stats: yStats,
      frame_count: annotatedFrames.length,
      args,
    },
  };
};

type OcrFramesBoxFilteredResult = {
  frames: FrameResult[];
  meta: OcrFramesBoxFilteredResultMeta;
};
type OcrFramesBoxFilteredResultMeta = {
  y_stats: YStats;
  frame_count: number;
};

export const get_ocr_frames_box_filtered = (
  ocrFramesBoxAdjustFrames: FrameResultBoxWithAdjust[],
): OcrFramesBoxFilteredResult => {
  const filteredFrames = ocrFramesBoxAdjustFrames.flatMap((f) => {
    const cleanBoxes = f.boxes.filter((a_box) => !a_box.is_outlier);
    if (cleanBoxes.length === 0) return [];
    if (cleanBoxes.length === f.boxes.length) return [f as FrameResult];
    const rebuilt = aggregate_boxes(cleanBoxes);
    return [
      {
        ...f,
        text: rebuilt.text,
        confidence: rebuilt.text_confidence,
        x_range: rebuilt.x_range,
        y_range: rebuilt.y_range,
        boxes: rebuilt.boxes,
      },
    ];
  });
  return {
    frames: filteredFrames,
    meta: {
      y_stats: computeBoxYStats(filteredFrames),
      frame_count: filteredFrames.length,
    },
  };
};
