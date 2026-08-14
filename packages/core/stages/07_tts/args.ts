import { z } from "zod";

export const TtsStageArgsSchema = z
  .object({
    runtime: z.enum(["ggml", "cloud", "voxcpm_torch_gradio"]).default("cloud"),
    device: z.enum(["webgpu", "cuda", "rocm", "cpu", "mps"]).default("cuda"),
    skipExisting: z.boolean().default(true),
    onlyIndices: z
      .array(z.number().int().positive())
      .optional()
      .describe("仅处理指定索引的 segment（其余跳过），可用于精准重跑指定段"),
    refAudioX2: z
      .boolean()
      .default(false)
      .describe("将短参考音频（< 2500ms）拼接一倍再送 TTS，帮助稳定输出音色"),
  })
  .prefault({})
  .describe(`input: 1. split_audio/timings.json: translation[i].dst`);
export type TtsArgs = z.output<typeof TtsStageArgsSchema>;
