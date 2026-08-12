import { TtsStageArgsSchema } from "@repo/tts/args";
import { ServersArgsSchema } from "../servers/args";
import { EnvArgsSchema } from "@repo/core/cmd/env/input";
import { z } from "zod";

import { LlmFixArgsSchema } from "@repo/llm/llm_fix_args";
import { langList, taskArgsSchema } from "@repo/core/cmd/tasks/input";
import { CookieArgsSchema } from "@repo/core/cmd/cookie/input";
import {
  OcrSegmentAdjustArgsSchema,
  MergeFramesArgsSchema,
  BoxAdjustedArgsSchema,
  AsrOcrFixArgsSchema,
  OcrFixArgsSchema,
} from "@repo/subtitle-ocr/args";
import { AsrCliArgsSchema } from "@repo/subtitle-asr/args";
import { SplitAudioArgsSchema } from "../stages/06_split_audio/args";

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

const SeparateCliInputSchema = z
  .object({
    runtime: z.enum(["ggml", "ort", "pytorch", "burn", "burn-tch"]),
    device: z
      .enum(["vulkan", "webgpu", "cuda", "cpu", "mps"])
      .default("cuda")
      .describe("pytorch:cuda (NVIDIA/ROCm), mps (Apple Silicon)"),
    always: z
      .boolean()
      .default(false)
      .describe(
        "效果(默认关闭): subtitle 模式下也始终分离人声，保留 vocals 以便后续切换到 dub; dub 流程下始终 需要分离人声以 保证 tts-vc 的质量",
      )
      .optional(),
    stems: z
      .array(z.enum(["drums", "bass", "other", "vocals"]))
      .default(["vocals"])
      .describe("需分离的 stems; 默认只分离 vocals, 目前仅支持 ort")
      .optional(),
  })
  .default({
    runtime: "pytorch",
    device: "cuda",
  })
  .optional()
  .describe(`separate: demucs 分离人声与背景声, 提升 tts-vc 的质量`);
export type SeparateConfig = z.infer<typeof SeparateCliInputSchema>;

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

const TranslateCliInputSchema = z
  .looseObject({
    apiBase: z.string().optional(),
    model: z.string().optional(),
    targetLang: z
      .enum(langList)
      .optional()
      .describe("如果不填则 按照这个逻辑: 源语言: zh -> en, 否则 any -> zh"), //
    enabled: z.boolean().optional().describe("设为 false 跳过翻译，直接使用原始识别文本"),
  })
  .optional();

const alignmentList = [
  "bottom-left",
  "bottom-center",
  "bottom-right",
  "middle-left",
  "center",
  "middle-right",
  "top-left",
  "top-center",
  "top-right",
] as const;
type Alignment = (typeof alignmentList)[number];
const AlignmentSchema = z.enum(alignmentList).default("bottom-center");

const ALIGNMENT_MAP: Record<Alignment, number> = Object.fromEntries(
  alignmentList.map((key, i) => [key, i + 1]),
) as Record<Alignment, number>;

export function alignmentToFfmpeg(alignment: Alignment): number {
  return ALIGNMENT_MAP[alignment] ?? 2;
}

const MergeVideoSchema = z
  .object({
    fontSize: z
      .number()
      .min(1)
      .max(200)
      .nullish()
      .describe("字幕字号，不填则自动: 竖屏: 12(zh) / 9(其他) ← 横屏: 24(zh) / 18(其他)"),
    marginV: z.number().min(0).nullish().describe("垂直边距(像素)，不填则自动: 竖屏 70 / 横屏 5"),
    alignment: AlignmentSchema.optional(),
    outline: z.number().min(0).default(0).optional(),
    shadow: z.number().min(0).default(1).optional(),
    font: z.string().optional().describe("ASS 字幕字体名（须系统已安装），默认 Noto Sans CJK SC"),
    srtPath: z.string().optional().describe("调试使用"),
    bgmPath: z.string().optional().describe("调试使用"),
    bgmGain: z.number().default(-6).optional().describe("背景音乐增益(dB), 0=不变, 负值=衰减"),
    dubGain: z.number().default(3).optional().describe("配音增益(dB), 补偿合成语音偏小的听感差"),
  })
  .default({
    alignment: "bottom-center",
    outline: 0,
    shadow: 1,
    bgmGain: -6,
    dubGain: 3,
  })
  .optional();

export type MergeVideoConfig = z.output<typeof MergeVideoSchema>;

const StagesSchema = z
  .object({
    separate: SeparateCliInputSchema,
    asr: AsrCliArgsSchema,
    asr_fix: z
      .looseObject({
        ...LlmFixArgsSchema.shape,
        segmentPad: z
          .boolean()
          .default(true)
          .describe("是否对 ASR 段落添加时间轴 padding")
          .optional(),
        asrFilePath: z.string().optional().describe("ASR 结果文件路径, 调试使用"),
      })
      .default({
        llmFix: false,
        segmentPad: true,
      } as any)
      .optional(),

    sf_ocr: SfOcrArgsSchema,
    sf_ocr_fix: OcrFixArgsSchema.prefault({}),
    asr_ocr: AsrOcrCliInputSchema,
    asr_ocr_fix: AsrOcrFixArgsSchema.prefault({}),
    translate: TranslateCliInputSchema,
    split_audio: SplitAudioArgsSchema,
    tts: TtsStageArgsSchema,
    merge_audio: z
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
      .prefault({}),
    merge_video: MergeVideoSchema,
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
