import { LlmFixArgsSchema } from "@repo/llm/llm_fix_args";
import { z } from "zod/v4";

export const AsrFixArgsSchema = z
  .looseObject({
    ...LlmFixArgsSchema.shape,
    asrFilePath: z.string().optional().describe("ASR 结果文件路径, 调试使用"),
  })
  .prefault({});
