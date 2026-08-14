import { z } from "zod/v4";

export const MergeAudioArgsSchema = z
  .object({
    maxSpeed: z.number().min(1).default(1.35).describe("TTS 音频最大变速比, 1.0=不变速"),
    maxAdvanceMs: z
      .number()
      .min(0)
      .default(500)
      .describe("字幕允许提前显示的最大毫秒数, 利用前段剩余时间"),
    maxDelayMs: z
      .number()
      .min(0)
      .default(500)
      .describe("字幕允许延迟显示的最大毫秒数, 借用后段留白"),
  })
  .prefault({});
