import { z } from "zod";

export const MergeFramesArgsSchema = z.object({
  mergeSubstring: z.boolean().default(false).optional(),
  dedupLevenshtein: z
    .number()
    .default(1)
    .describe("dedupOverlap 的编辑距离阈值: levenshtein ≤ 此值则合并; 默认 1")
    .optional(),
});
export type MergeFramesArgs = z.output<typeof MergeFramesArgsSchema>;

export const BoxAdjustedArgsSchema = z.object({
  boxAdjustedThreshold: z
    .number()
    .default(0.5)
    .describe("box调整的置信度阈值: confidence < 此值则进行box调整; 默认 0.5")
    .optional(),
});
export type BoxAdjustedArgs = z.output<typeof BoxAdjustedArgsSchema>;
