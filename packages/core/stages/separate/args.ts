import { z } from "zod/v4";

export const SeparateArgsSchema = z
  .object({
    runtime: z.enum(["burn", "burn-tch"]).default("burn-tch"),
    device: z
      .enum(["vulkan", "webgpu", "cuda", "cpu", "mps"])
      .default("cuda")
      .describe("torch:cuda (NVIDIA/ROCm), mps (Apple Silicon)"),
    always: z
      .boolean()
      .default(false)
      .describe(
        "效果(默认关闭): subtitle 模式下也始终分离人声，保留 vocals 以便后续切换到 dub; dub 流程下始终 需要分离人声以 保证 tts-vc 的质量",
      ),
    stems: z
      .array(z.enum(["drums", "bass", "other", "vocals"]))
      .default([])
      .describe("需分离的 stems; 暂不被消费"),
  })
  .prefault({})
  .describe(`separate: demucs 分离人声与背景声, 提升 tts-vc 的质量`);
