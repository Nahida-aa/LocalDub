import { z } from "zod";

export const AsrArgsSchema = z
  .looseObject({
    runtime: z.enum(["ggml", "faster-whisper", "pytorch"]).default("ggml"),
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

export type AsrArgs = z.output<typeof AsrArgsSchema>;
