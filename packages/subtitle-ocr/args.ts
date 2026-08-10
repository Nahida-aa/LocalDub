import { z } from "zod";
import { LlmFixArgsSchema } from "@repo/llm/llm_fix_args";

export const OcrSegmentAdjustArgsSchema = z.object({
  isoThresholdMs: z
    .number()
    .default(1500)
    .describe("单帧孤立惩罚的参考时间 (ms)，在此时长内无同文帧则视为完全孤立; 默认 1500"),
  adjustYWeight: z.number().default(0.8).describe("Y 偏移在调整置信度中的权重 (0~1); 默认 0.8"),
  adjustIsoWeight: z.number().default(0.2).describe("孤立程度在调整置信度中的权重 (0~1); 默认 0.2"),
  adjustYFactor: z
    .number()
    .default(0.08)
    .describe(
      "Y 偏移惩罚归一化系数: 偏移量 / (videoHeight × adjustYFactor); 越小越严格; 默认 0.08",
    ),
});
export type OcrSegmentAdjustArgs = z.output<typeof OcrSegmentAdjustArgsSchema>;

export const MergeFramesArgsSchema = z.object({
  is_merge_substring: z.boolean().default(false),
  dedup_edit_distance: z
    .number()
    .default(1)
    .describe("dedupOverlap 的编辑距离阈值: edit_distance ≤ 此值则合并; 默认 1"),
});
export type MergeFramesArgs = z.output<typeof MergeFramesArgsSchema>;

export const BoxAdjustedArgsSchema = z.object({
  boxAdjustedThreshold: z
    .number()
    .default(0.5)
    .describe("box调整的置信度阈值: confidence < 此值则进行box调整; 默认 0.5"),
});
export type BoxAdjustedArgs = z.output<typeof BoxAdjustedArgsSchema>;

export const OcrFixArgsSchema = z.looseObject({
  adjusted_confidence_threshold: z
    .number()
    .default(0.45)
    .describe(
      "字幕段置信度阈值（0-1）：ocr-post filter-segment 用 adjusted_confidence_threshold 过滤，低于此值丢弃；默认 0.5",
    ),
  ...OcrSegmentAdjustArgsSchema.shape,
  ...BoxAdjustedArgsSchema.shape,
  ...MergeFramesArgsSchema.shape,
  ...LlmFixArgsSchema.shape,
});

export type OcrFixArgs = z.output<typeof OcrFixArgsSchema>;

export const AsrOcrFixArgsSchema = z.looseObject({
  ...OcrFixArgsSchema.shape,
  is_resample: z.boolean().default(false),
});

export type AsrOcrFixArgs = z.output<typeof AsrOcrFixArgsSchema>;
