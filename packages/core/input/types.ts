import { TtsStageInputSchema } from "@repo/core/input/tts";
import { ServersArgsSchema } from "@repo/core/servers/input";
import { EnvArgsSchema } from "@repo/core/cmd/env/input";
import { z } from "zod";
import { LineAdjustedArgsSchema, MergeFramesArgsSchema } from "@repo/core/ml/subtitle_ocr/input";
import { LlmFixArgsSchema } from "@repo/core/ml/llm/input";
import { langList, taskArgsSchema } from "@repo/core/cmd/tasks/input";
import { CookieArgsSchema } from "@repo/core/cmd/cookie/input";

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

const ocrRuntimeList = ["ort-cpp", "ort-node", "ort-py", "ort-rust"] as const;
const ocrRuntimeSchema = z
  .enum(ocrRuntimeList)
  .default("ort-cpp")
  .describe(
    "OCR 推理运行时: ort-cpp (C++ + OpenCV 预处理), ort-node (onnxruntime-node), ort-py (Python rapidocr), ort-rust (Rust 二进制)",
  )
  .optional();
export const ocrAfterAdjustArgsSchema = z.object({
  isoThresholdMs: z
    .number()
    .default(1500)
    .describe("单帧孤立惩罚的参考时间 (ms)，在此时长内无同文帧则视为完全孤立; 默认 1500")
    .optional(),
  adjustYWeight: z
    .number()
    .default(0.8)
    .describe("Y 偏移在调整置信度中的权重 (0~1); 默认 0.8")
    .optional(),
  adjustIsoWeight: z
    .number()
    .default(0.2)
    .describe("孤立程度在调整置信度中的权重 (0~1); 默认 0.2")
    .optional(),
  adjustYFactor: z
    .number()
    .default(0.08)
    .describe("Y 偏移惩罚归一化系数: 偏移量 / (videoHeight × adjustYFactor); 越小越严格; 默认 0.08")
    .optional(),
  lineAdjustedThreshold: z
    .number()
    .default(0.5)
    .describe("行级 outlier 判定: adjustedConfidence < 此值则标记为 outlier; 默认 0.5")
    .optional(),
});
export type OcrAfterAdjustArgs = z.output<typeof ocrAfterAdjustArgsSchema>;
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
    textScore: z.number().default(0.45).describe("OCR 识别置信度阈值, 默认 0.45").optional(),
    subtitleOnly: z
      .boolean()
      .default(true)
      .describe("只识别字幕区域 (Y轴裁剪); 默认 true")
      .optional(),
    yRange: z
      .tuple([z.number(), z.number()])
      .describe(
        "自定义字幕识别区域 [y_min, y_max] (像素, 相对原始帧); 设置后优先于 subtitleOnly 硬编码区域",
      )
      .optional(),
    cleanupFrames: z
      .boolean()
      .default(false)
      .describe("步骤完成后是否删除抽出的帧图片; 默认 false (保留)")
      .optional(),
    ...ocrAfterAdjustArgsSchema.shape,
    ...MergeFramesArgsSchema.shape,
  })
  .default({
    runtime: "ort-cpp",
    device: "cpu",
    fps: 2,
    textScore: 0.45,
    subtitleOnly: true,
    cleanupFrames: false,
    isoThresholdMs: 1500,
    adjustYWeight: 0.8,
    adjustIsoWeight: 0.2,
    adjustYFactor: 0.08,
  })
  .optional();
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
    textScore: z.number().default(0.45).describe("OCR 识别置信度阈值, 默认 0.45").optional(),
    subtitleOnly: z
      .boolean()
      .default(true)
      .describe("只识别字幕区域 (Y轴裁剪); 默认 true")
      .optional(),
    yRange: z
      .tuple([z.number(), z.number()])
      .describe(
        "自定义字幕识别区域 [y_min, y_max] (像素, 相对原始帧); 设置后优先于 subtitleOnly 硬编码区域",
      )
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
    textScore: 0.45,
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

    import_subtitle: z
      .looseObject({
        file: z
          .string()
          .optional()
          .describe(
            '外部字幕文件路径 (VTT/SRT), 优先级高于 task.subtitleFile; 需配合 task.subtitleSource: "file"',
          ),
        skipSeparate: z
          .boolean()
          .default(false)
          .optional()
          .describe(
            "跳过 demucs 人声分离 (separate/separate_after), 会降低 TTS 参考音质量; 仅 dub 模式生效, subtitle 模式本就分离",
          ),
        segmentPad: z
          .boolean()
          .default(true)
          .describe("是否对导入的字幕段落添加时间轴 padding")
          .optional(),
        mergeOverlap: z
          .boolean()
          .optional()
          .describe(
            '合并相邻重叠字幕段并按句子重切，用于清理 YouTube 自动字幕的渐进式滑动窗口重叠（每句重复上句尾部 + 10ms 回声段）；默认仅当 task.subtitleSource === "file" 时启用，对无重叠的干净字幕幂等',
          ),
        maxSegmentWords: z
          .number()
          .int()
          .positive()
          .optional()
          .describe(
            "TTS 配音段最大词数（默认 10）：长句优先在逗号处切分，无逗号则从后往前按词边界切分，保证每段不超过该词数",
          ),
      })
      .default({} as any)
      .optional(),

    ocr: OcrCliInputSchema,
    asr_ocr: AsrOcrCliInputSchema,
    asr_ocr_fix: z
      .looseObject({
        textScore: z
          .number()
          .default(0.5)
          .optional()
          .describe("OCR 文本置信度阈值（0-1），低于此阈值的帧在合并前会被丢弃"),
        ...ocrAfterAdjustArgsSchema.shape,
        ...LineAdjustedArgsSchema.shape,
        ...MergeFramesArgsSchema.shape,
        ...LlmFixArgsSchema.shape,
      })
      .default({
        llmFix: false,
        textScore: 0.5,
        isoThresholdMs: 1500,
        adjustYWeight: 0.8,
        adjustIsoWeight: 0.2,
        adjustYFactor: 0.08,
      } as any)
      .optional(),
    ocr_fix: z
      .looseObject({
        ...LlmFixArgsSchema.shape,
      })
      .default({} as any),
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
