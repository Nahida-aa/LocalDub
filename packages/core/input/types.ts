import { TtsStageInputSchema } from "@repo/core/input/tts";
import { ServersArgsSchema } from "@repo/core/servers/input";
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

const ASRCliInputSchema = z
  .looseObject({
    runtime: z.enum(["ggml", "faster-whisper", "pytorch"]).default("pytorch"),
    device: z.enum(["vulkan", "cuda", "cpu", "mps"]).default("cuda"),
    useSeparated: z
      .boolean()
      .default(false)
      .describe("使用分离后的人声 (target_3_vocals.wav) 而非原始视频音频")
      .optional(),
    mixMode: z
      .enum(["vocals", "raw-sum", "sidechain"])
      .default("sidechain")
      .describe(`ASR 音频源: vocals=纯分离人声,
				raw-sum=人声+降低背景音直接叠加,
				sidechain=人声+侧链压缩背景音`)
      .optional(),
    reduceBgm: z
      .number()
      .default(-12)
      .describe("背景音降低量(dB); raw-sum 时叠加前直接衰减, sidechain 时压缩后额外衰减")
      .optional(),
    wordsOutput: z
      .boolean()
      .default(false)
      .describe(
        "是否在 asr.json 中包含词级时间戳 (words), 分离场景下可能受幻觉影响；默认关闭，调试时开启",
      )
      .optional(),
    sidechainCompress: z
      .object({
        threshold: z.number().default(0.1).describe("压缩器阈值, 默认 0.1"),
        ratio: z.number().default(20).describe("压缩比, 默认 20"),
        attack: z.number().default(1).describe("attack 时间(ms), 默认 1"),
        release: z.number().default(200).describe("release 时间(ms), 默认 200"),
      })
      .default({
        threshold: 0.1,
        ratio: 20,
        attack: 1,
        release: 200,
      })
      .describe("mixMode=sidechain 时侧链压缩器参数")
      .optional(),
    useGate: z
      .boolean()
      .default(false)
      .describe("对分离后的人声应用 silence gate 过滤静音段噪声")
      .optional(),
    vocalAudioPath: z.string().optional().describe("ASR 输入的人声音频路径, 调试使用"),
    // whisper.cpp specific params (ignored by other runtimes)
    vad: z.boolean().default(false).optional().describe("whisper.cpp: 启用 VAD"),
    vadModel: z
      .enum(["silero-v5", "silero-v6"])
      .optional()
      .describe("whisper.cpp: VAD 模型, silero-v5 (默认) 或 silero-v6"),
    vadThreshold: z
      .number()
      .min(0)
      .max(1)
      .default(0.5)
      .optional()
      .describe("whisper.cpp: VAD 阈值, 默认 0.5"),
    noSpeechThold: z
      .number()
      .min(0)
      .default(0.6)
      .optional()
      .describe("whisper.cpp: no-speech 阈值, 默认 0.6"),
    temperature: z
      .number()
      .min(0)
      .max(2)
      .default(0.0)
      .optional()
      .describe("whisper.cpp: 解码温度, 默认 0.0"),
    maxLen: z
      .number()
      .int()
      .min(0)
      .default(0)
      .optional()
      .describe("whisper.cpp: 最大段长(字符), 0=不限"),
    splitOnWord: z.boolean().default(false).optional().describe("whisper.cpp: 按词边界分割"),
  })
  .default({
    runtime: "pytorch",
    device: "cuda",
    useSeparated: false,
    mixMode: "sidechain",
    reduceBgm: -12,
    wordsOutput: false,
    sidechainCompress: { threshold: 0.1, ratio: 20, attack: 1, release: 200 },
    useGate: false,
  })
  .optional();

export type ASRConfig = z.output<typeof ASRCliInputSchema>;

const ocrRuntimeList = ["ort-rust", "ort-cpp", "ort-node", "ort-py"] as const;
const ocrRuntimeSchema = z
  .enum(ocrRuntimeList)
  .default("ort-cpp")
  .describe(
    "OCR 推理运行时: ort-cpp (C++ + OpenCV 预处理), ort-node (onnxruntime-node), ort-py (Python rapidocr)",
  )
  .optional();

const OcrCliInputSchema = z
  .looseObject({
    runtime: ocrRuntimeSchema,
    device: z
      .enum(["cpu", "cuda", "directml", "coreml", "rocm", "mps"])
      .default("cpu")
      .describe(
        "OCR 运行设备: cpu, cuda (NVIDIA), directml (Windows), coreml (macOS), rocm (AMD), mps (Apple Silicon)",
      )
      .optional(),
    fps: z.number().default(2).describe("帧率 (fps), 越高时间戳越准但越慢; 默认 2").optional(),
    text_score_threshold: z
      .number()
      .default(0.45)
      .describe("OCR 识别置信度阈值, 默认 0.45")
      .optional(),
    subtitleOnly: z
      .boolean()
      .default(true)
      .describe("只识别字幕区域 (Y轴裁剪); 默认 true")
      .optional(),
    cleanupFrames: z
      .boolean()
      .default(false)
      .describe("步骤完成后是否删除抽出的帧图片; 默认 false (保留)")
      .optional(),
    ...OcrSegmentAdjustArgsSchema.shape,
    ...MergeFramesArgsSchema.shape,
  })
  .default({} as any);
export type OcrConfig = z.output<typeof OcrCliInputSchema>;

const AsrOcrCliInputSchema = z
  .looseObject({
    runtime: ocrRuntimeSchema,
    device: z
      .enum(["cpu", "cuda", "directml", "coreml", "rocm", "mps"])
      .default("cpu")
      .describe(
        "OCR 运行设备: cpu, cuda (NVIDIA), directml (Windows), coreml (macOS), rocm (AMD), mps (Apple Silicon)",
      )
      .optional(),
    fps: z.number().default(2).describe("帧率 (fps), 越高时间戳越准但越慢; 默认 2").optional(),
    text_score_threshold: z
      .number()
      .default(0.45)
      .describe("OCR 识别置信度阈值, 默认 0.45")
      .optional(),
    subtitleOnly: z
      .boolean()
      .default(true)
      .describe("只识别字幕区域 (Y轴裁剪); 默认 true")
      .optional(),
    cleanupFrames: z
      .boolean()
      .default(false)
      .describe("步骤完成后是否删除抽出的帧图片; 默认 false (保留)")
      .optional(),
  })
  .default({
    runtime: "ort-cpp",
    device: "cpu",
    fps: 2,
    text_score_threshold: 0.45,
    subtitleOnly: true,
    cleanupFrames: false,
  });

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

const SplitAudioCliInputSchema = z
  .looseObject({
    vadAlign: z
      .boolean()
      .default(false)
      .describe("是否启用静音检测对齐: 修正 segments 前后静音导致的偏移")
      .optional(),
    vocalsFilePath: z.string().optional().describe("人声文件路径, 调试使用"),
    sourceFilePath: z.string().optional().describe("原始视频音频路径, 调试使用"),
  })
  .default({
    vadAlign: false,
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
    asr: ASRCliInputSchema,
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

    ocr: OcrCliInputSchema,
    asr_ocr: AsrOcrCliInputSchema,
    asr_ocr_fix: AsrOcrFixArgsSchema.default({} as any),
    ocr_fix: OcrFixArgsSchema.default({} as any),
    translate: TranslateCliInputSchema,
    split_audio: SplitAudioCliInputSchema,
    tts: TtsStageInputSchema,
    merge_audio: z
      .object({
        maxSpeed: z
          .number()
          .min(1)
          .default(1.35)
          .optional()
          .describe("TTS 音频最大变速比, 1.0=不变速"),
        maxAdvanceMs: z
          .number()
          .min(0)
          .default(500)
          .optional()
          .describe("字幕允许提前显示的最大毫秒数, 利用前段剩余时间"),
        maxDelayMs: z
          .number()
          .min(0)
          .default(500)
          .optional()
          .describe("字幕允许延迟显示的最大毫秒数, 借用后段留白"),
      })
      .default({
        maxSpeed: 1.35,
        maxAdvanceMs: 500,
        maxDelayMs: 500,
      })
      .optional(),
    merge_video: MergeVideoSchema,
  })
  .default({} as any);

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
