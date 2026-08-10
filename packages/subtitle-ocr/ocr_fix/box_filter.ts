import { FrameResult, OcrBoxResult } from "../types";
import { YStats, computeBoxYStats } from "./stats";
import { AsrOcrFixArgs, BoxAdjustedArgs, MergeFramesArgs } from "@repo/subtitle-ocr/args";
import { aggregate_boxes } from "../ocr_util";
import { FrameResultBoxWithAdjust } from "./box_adjust";

export type OcrFramesBoxFilteredResult = {
  frames: FrameResult[];
  meta: OcrFramesBoxFilteredResultMeta;
};
type OcrFramesBoxFilteredResultMeta = {
  y_stats: YStats;
  frame_count: number;
};

export const ocr_frames_filter_box = (
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
        text_confidence: rebuilt.text_confidence,
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
