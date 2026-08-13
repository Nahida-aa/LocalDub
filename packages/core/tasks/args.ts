import { z } from "zod";
export const langList = [
  "en",
  "zh",
  "vi", // 越南语
  "ja",
  "ko",
  "fr",
  "de",
  "es",
  "pt",
  "ru",
  "ar",
  "hi",
  "th",
  "id",
  "ms",
  "tl",
  "my",
  "km",
  "lo",
  "mn",
  "ne",
  "ur",
  "bn",
] as const;
export type TargetLang = (typeof langList)[number];

const stagesList = [
  "separate",
  "separate_after",
  "asr",
  "asr_fix",
  "sf_ocr_pre",
  "sf_ocr",
  "sf_ocr_fix",
  "asr_ocr_pre",
  "asr_ocr",
  "asr_ocr_fix",
  "translate",
  "split_audio",
  "tts",
  "merge_audio",
  "merge_video",
] as const;
export type StageName = (typeof stagesList)[number];

export const subtitleSourceList = ["asr", "sf_ocr", "asr_ocr"] as const;
export type SubtitleSource = (typeof subtitleSourceList)[number];

const taskActionList = ["start", "continue", "status", "get_group_list", "get_task_ctx"] as const;
export const taskArgsSchema = z.object({
  action: z
    .enum(taskActionList)
    .optional()
    .describe("任务操作: start=开始, continue=继续, status=显示状态, get_group_list=列出分组"),
  url: z.string().optional().describe("本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl"),
  sourceLang: z.enum(langList).optional(),
  targetLang: z.enum(langList).optional(),
  continueFrom: z
    .enum(stagesList)
    .optional()
    .describe(`继续任务专业参数, 可指定 continueFrom 从某步骤开始, 不指定则从上次中断的步骤开始`),
  targetStage: z
    .enum(stagesList)
    .optional()
    .describe("目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤"),
  taskDir: z.string().optional(),
  stageName: z.enum(stagesList).optional().describe(`rerunStage 专业参数, 指定要重新运行的步骤`),
  pipeline: z
    .enum(["dub", "subtitle"])
    .default("dub")
    .optional()
    .describe("任务模式, dub 配音,subtitle 仅字幕"),
  subtitleSource: z
    .enum(subtitleSourceList)
    .default("asr")
    .optional()
    .describe(
      "字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)",
    ),
});
