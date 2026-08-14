import { TtsStageArgsSchema } from "../stages/07_tts/args";
import { ServersArgsSchema } from "../servers/args";
import { EnvArgsSchema } from "@repo/core/cmd/env/input";
import { z } from "zod";
import { SeparateArgsSchema } from "../stages/separate/args";

import { LlmFixArgsSchema } from "@repo/llm/llm_fix_args";
import { taskArgsSchema } from "../tasks/args";
import { langList } from "../const/lang";
import { CookieArgsSchema } from "@repo/core/cmd/cookie/input";
import {
  OcrSegmentAdjustArgsSchema,
  MergeFramesArgsSchema,
  BoxAdjustedArgsSchema,
  AsrOcrFixArgsSchema,
  OcrFixArgsSchema,
} from "@repo/subtitle-ocr/args";
import { AsrArgsSchema } from "@repo/subtitle-asr/args";
import { SplitAudioArgsSchema } from "../stages/06_split_audio/args";
import { TranslateArgsSchema } from "../stages/05_translate/args";
import { AsrFixArgsSchema } from "../stages/asr/fix_args";
import { MixAudioArgsSchema } from "../stages/mix_audio/args";
import { MixVideoArgsSchema } from "../stages/mix_video/args";

const deviceList = ["cpu", "cuda", "mps", "webgpu"] as const;
export type Device = (typeof deviceList)[number];

export const commandList = [
  "task",
  "check",
  "deviceInfo",
  "servers",
  "listModels",
  "env",
  "cookie",
] as const;

export type Command = (typeof commandList)[number];

const ocrRuntimeList = ["ort-rust"] as const;
const ocrRuntimeSchema = z
  .enum(ocrRuntimeList)
  .default("ort-rust")
  .describe("OCR 推理运行时: ort-rust (opencv)");

const SfOcrArgsSchema = z
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
    ...OcrSegmentAdjustArgsSchema.shape,
    ...MergeFramesArgsSchema.shape,
  })
  .prefault({});
export type SfOcrArgs = z.output<typeof SfOcrArgsSchema>;

const AsrOcrCliInputSchema = z
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

export type AsrOcrConfig = z.output<typeof AsrOcrCliInputSchema>;

const StagesSchema = z
  .object({
    separate: SeparateArgsSchema,
    asr: AsrArgsSchema,
    asr_fix: AsrFixArgsSchema,
    sf_ocr: SfOcrArgsSchema,
    sf_ocr_fix: OcrFixArgsSchema.prefault({}),
    asr_ocr: AsrOcrCliInputSchema,
    asr_ocr_fix: AsrOcrFixArgsSchema,
    translate: TranslateArgsSchema,
    split_audio: SplitAudioArgsSchema,
    tts: TtsStageArgsSchema,
    mix_audio: MixAudioArgsSchema,
    mix_video: MixVideoArgsSchema,
  })
  .prefault({});

export const CliInputSchema = z.looseObject({
  command: z
    .enum(commandList)
    .describe(`
		6. check: 检测某任务的结果 (如视频是否下载成功, ASR 结果是否合理等)
		7. deviceInfo: 显示设备信息
		8. servers: 统一管理所有服务器 (servers.action=status 查状态, stop 停止, start 启动; servers.name 指定单个)
		9. listModels: 列出 openai 兼容端点的 可用模型
		10. env: 环境检查/修复 (check=诊断, ensure=尝试修复)
		`)
    .default("env"),
  task: taskArgsSchema,
  check: z
    .object({
      taskDir: z.string().optional(),
      type: z.enum(["video", "asr", "font"]).optional().default("video"),
    })
    .optional(),
  servers: ServersArgsSchema,
  env: EnvArgsSchema.optional(),
  cookie: CookieArgsSchema.optional(),
  stages: StagesSchema,
});
export type CliInputInput = z.input<typeof CliInputSchema>;
export type CliInput = z.output<typeof CliInputSchema>;
