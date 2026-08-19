import { TtsStageArgsSchema } from "../stages/07_tts/args";
import { ServersArgsSchema } from "../servers/args";
import { EnvArgsSchema } from "@repo/core/cmd/env/input";
import { z } from "zod";
import { SeparateArgsSchema } from "../stages/separate/args";

import { LlmFixArgsSchema } from "@repo/llm/llm_fix_args";
import { taskArgsSchema } from "../tasks/args";
import { langList } from "../const/lang";
import { CookieArgsSchema } from "../cmd/cookie/args";

import { AsrArgsSchema } from "@repo/subtitle-asr/args";
import { SplitAudioArgsSchema } from "../stages/06_split_audio/args";
import { TranslateArgsSchema } from "../stages/05_translate/args";
import { AsrFixArgsSchema } from "../stages/asr/fix_args";
import { MixAudioArgsSchema } from "../stages/mix_audio/args";
import { MixVideoArgsSchema } from "../stages/mix_video/args";
import { AsrOcrPreArgsSchema } from "../stages/04_asr_ocr/pre_args";
import { OcrFixArgsSchema } from "../stages/sf_ocr/fix_args";
import { AsrOcrFixArgsSchema } from "../stages/04_asr_ocr/fix_args";
import { AsrOcrArgsSchema } from "../stages/04_asr_ocr/args";
import { SfOcrArgsSchema } from "../stages/sf_ocr/args";

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

const StagesSchema = z
  .object({
    separate: SeparateArgsSchema,
    asr: AsrArgsSchema,
    asr_fix: AsrFixArgsSchema,
    sf_ocr: SfOcrArgsSchema,
    sf_ocr_fix: OcrFixArgsSchema.prefault({}),
    asr_ocr_pre: AsrOcrPreArgsSchema,
    asr_ocr: AsrOcrArgsSchema,
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
