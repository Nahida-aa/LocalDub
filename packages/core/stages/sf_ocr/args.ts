import z from "zod";

const ocrRuntimeList = ["ort-rust"] as const;
export const ocrRuntimeSchema = z
  .enum(ocrRuntimeList)
  .default("ort-rust")
  .describe("OCR 推理运行时: ort-rust (opencv)");

export const SfOcrArgsSchema = z
  .looseObject({
    runtime: ocrRuntimeSchema,
    device: z
      .enum(["cpu", "cuda", "directml", "coreml", "rocm", "mps"])
      .default("cpu")
      .describe(
        "OCR 运行设备: cpu, cuda (NVIDIA), directml (Windows), coreml (macOS), rocm (AMD), mps (Apple Silicon)",
      ),
    text_confidence_threshold: z.number().default(0.45).describe("OCR 识别置信度阈值, 默认 0.45"),
    subtitleOnly: z.boolean().default(true).describe("只识别字幕区域 (Y轴裁剪); 默认 true"),
    cleanupFrames: z
      .boolean()
      .default(false)
      .describe("步骤完成后是否删除抽出的帧图片; 默认 false (保留)"),
  })
  .prefault({});
