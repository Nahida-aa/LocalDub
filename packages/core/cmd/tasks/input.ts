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
  "import_subtitle",
  "ocr",
  "ocr_fix",
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
export enum StageNameEnum {
  separate = "separate",
  separate_after = "separate_after",
  asr = "asr",
  asr_fix = "asr_fix",
  import_subtitle = "import_subtitle",
  ocr = "ocr",
  ocr_fix = "ocr_fix",
  asr_ocr_pre = "asr_ocr_pre",
  asr_ocr = "asr_ocr",
  asr_ocr_fix = "asr_ocr_fix",
  translate = "translate",
  split_audio = "split_audio",
  tts = "tts",
  merge_audio = "merge_audio",
  merge_video = "merge_video",
}

export const subtitleSourceList = ["asr", "ocr", "asr_ocr", "file"] as const;
export type SubtitleSource = (typeof subtitleSourceList)[number];

const taskActionList = [
  "start",
  "resume",
  "rerun_stage",
  "status",
  "get_group_list",
  "get_task_ctx",
] as const;
export const taskArgsSchema = z.object({
  action: z
    .enum(taskActionList)
    .optional()
    .describe(
      "任务操作: start=开始, resume=继续, rerun_stage=重新运行某步骤, status=显示状态, get_group_list=列出分组",
    ),
  url: z.string().optional().describe("本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl"),
  sourceLang: z.enum(langList).optional(),
  targetLang: z.enum(langList).optional(),
  resumeFrom: z
    .enum(stagesList)
    .optional()
    .describe(`继续任务专业参数, 可指定 resumeFrom 从某步骤开始, 不指定则从上次中断的步骤开始`),
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
      "字幕源: asr (whisper, 默认), ocr (RapidOCR 硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合), file (导入外部 VTT/SRT 字幕文件)",
    ),
  subtitleFile: z
    .string()
    .optional()
    .describe(
      '外部字幕文件路径 (VTT/SRT), 配合 subtitleSource: "file" 使用, 也可在 stages.import_subtitle.file 指定',
    ),
  targetStage: z
    .enum(stagesList)
    .optional()
    .describe("目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤"),
});
