import { z } from "zod";

export const TtsStageArgsSchema = z
  .object({
    runtime: z.enum(["ggml", "cloud", "voxcpm_torch_gradio"]).default("cloud"),
    device: z.enum(["webgpu", "cuda", "rocm", "cpu", "mps"]).default("cuda"),
    skipExisting: z.boolean().default(true),
    regenIndices: z
      .array(z.number().int().positive())
      .optional()
      .describe(
        "continue 模式下强制重新生成的 segment 索引（1-based）；列表外段保留旧结果。命中段无视 skipExisting，强制重合成",
      ),
    refAudioX2: z
      .boolean()
      .default(false)
      .describe("将短参考音频（< 2500ms）拼接一倍再送 TTS，帮助稳定输出音色"),
  })
  .prefault({})
  .describe(`input: 1. split_audio/timings.json: translation[i].dst`);
export type TtsArgs = z.output<typeof TtsStageArgsSchema>;
