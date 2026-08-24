import z from "zod";
import { ocrRuntimeSchema } from "../sf_ocr/args";

export const AsrOcrArgsSchema = z
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
    yRange: z
      .tuple([z.number(), z.number()])
      .optional()
      .describe(
        "Y 轴像素过滤范围 [y1, y2] (原图坐标), 仅保留该区间内的字幕框; 默认不限制。指定后替代 subtitleOnly 的 [0.85, 0.99] 比例硬编码过滤",
      ),
    cleanupFrames: z
      .boolean()
      .default(false)
      .describe("步骤完成后是否删除抽出的帧图片; 默认 false (保留)"),
  })
  .prefault({});
