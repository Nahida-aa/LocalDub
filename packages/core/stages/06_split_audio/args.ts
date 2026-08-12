import { z } from "zod/v4";

export const SplitAudioArgsSchema = z
  .looseObject({
    vadAlign: z
      .boolean()
      .default(false)
      .describe("是否启用静音检测对齐: 修正 segments 前后静音导致的偏移"),
    vocalsFilePath: z.string().optional().describe("人声文件路径, 调试使用"),
    sourceFilePath: z.string().optional().describe("原始视频音频路径, 调试使用"),
  })
  .prefault({});
