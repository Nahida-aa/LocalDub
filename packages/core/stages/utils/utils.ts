import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { env } from "@repo/config/env";
import { getStages } from "./stages";
import { WHISPER_MODEL_DIR } from "@repo/config/path/models";

import {
  _readCtx,
  TaskCtx,
  getTaskId,
  listStage,
  readCtx,
  Task,
} from "@repo/core/context/context.ts";
import { SubtitleSource, TargetLang } from "@repo/core/cmd/tasks/input";
import { readJson } from "../../utils/fileOps";
import { TranslateFile } from "../05_translate/type";
import { SplitAudioFile, SplitAudioTimingFile } from "../06_split_audio/types";
import { TaskStage } from "../../context/types";
import { TimingsFile } from "../merge_audio/types";

/** Get the downloaded video source path for a session. */
export function video_source_path(ctx: TaskCtx): string {
  if (!ctx.video_source_path) {
    throw new Error(`video_source_path not set in context for session ${ctx.task.task_dir}`);
  }
  return ctx.video_source_path;
}

/** Get the vocals stem path from separate stage. */
export function vocalsPath(taskDir: string): string {
  return join(taskDir, "separate", "target_3_vocals.wav");
}

/** Get the BGM stem path from separate_after stage. */
export function bgmPath(taskDir: string): string {
  return join(taskDir, "separate_after", "target_bgm.wav");
}

/** Get the separate stage output directory. */
export function separateDir(taskDir: string): string {
  return join(taskDir, "separate");
}

/** Get the ASR output directory. */
export function asrDir(taskDir: string): string {
  return join(taskDir, "asr");
}

/** Get the separate_after output directory. */
export function separateAfterDir(taskDir: string): string {
  return join(taskDir, "separate_after");
}

export function defaultFont(dstLang: string): string {
  if (dstLang !== "zh") return "Arial";
  switch (process.platform) {
    case "win32":
      return "Microsoft YaHei";
    case "darwin":
      return "PingFang SC";
    default:
      return "Noto Sans CJK SC";
  }
}

export function nowISO(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "");
}

export function probeVideoResolution(videoPath: string): { width: number; height: number } {
  const r = spawnSync(
    "ffprobe",
    [
      "-v",
      "error",
      "-select_streams",
      "v:0",
      "-show_entries",
      "stream=width,height",
      "-of",
      "csv=p=0",
      videoPath,
    ],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  const [w, h] = r.stdout.toString().trim().split(",").map(Number);
  return { width: w, height: h };
}

export function probeSampleRate(audioPath: string): number {
  const r = spawnSync(
    "ffprobe",
    ["-v", "error", "-show_entries", "stream=sample_rate", "-of", "csv=p=0", audioPath],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  return parseInt(r.stdout.toString().trim()) || 48000;
}

export function probeDuration(audioPath: string): number {
  const r = spawnSync(
    "ffprobe",
    ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", audioPath],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  return parseFloat(r.stdout.toString().trim()) || 0;
}

function ffmpegInstallHint(): string {
  switch (process.platform) {
    case "win32":
      return "winget install Gyan.FFmpeg";
    case "darwin":
      return "brew install ffmpeg";
    case "linux": {
      if (existsSync("/usr/bin/apt-get")) return "sudo apt install -y ffmpeg";
      if (existsSync("/usr/bin/pacman")) return "sudo pacman -S ffmpeg";
      if (existsSync("/sbin/apk")) return "apk add ffmpeg";
      return "sudo apt install -y ffmpeg";
    }
    default:
      return "sudo apt install -y ffmpeg";
  }
}

// export async function updateTaskDB(
// 	taskId: string,
// 	fields: Record<string, unknown>,
// ) {
// 	if (Object.keys(fields).length === 0) return;
// 	await db.update(tasks).set(fields).where(eq(tasks.id, taskId));
// 	broadcastUpdate('tasks', [
// 		{ type: 'update', id: taskId, data: fields as any },
// 	]);
// }

// export async function updateStageDB(
// 	taskId: string,
// 	name: string,
// 	fields: Record<string, unknown>,
// ) {
// 	if (Object.keys(fields).length === 0) return;
// 	await db
// 		.update(taskStages)
// 		.set(fields)
// 		.where(
// 			sql`${taskStages.task_id} = ${taskId} AND ${taskStages.name} = ${name}`,
// 		);
// 	broadcastUpdate('task_stages', [
// 		{
// 			type: 'update',
// 			id: `${taskId}_${name}`,
// 			data: { task_id: taskId, name, ...fields } as any,
// 		},
// 	]);
// }

export const LANG_NAMES: Record<string, string> = {
  en: "English",
  zh: "Chinese",
  vi: "Vietnamese",
  ja: "Japanese",
  ko: "Korean",
  fr: "French",
  de: "German",
  es: "Spanish",
  pt: "Portuguese",
  ru: "Russian",
  ar: "Arabic",
  hi: "Hindi",
  th: "Thai",
  id: "Indonesian",
  ms: "Malay",
  tl: "Tagalog",
  my: "Burmese",
  km: "Khmer",
  lo: "Lao",
  mn: "Mongolian",
  ne: "Nepali",
  ur: "Urdu",
  bn: "Bengali",
};

export function readTaskLanguages(ctx: TaskCtx): {
  asrLanguage: string;
  targetLanguage: TargetLang;
} {
  if (ctx) {
    return {
      asrLanguage: ctx.asr_language || "en",
      targetLanguage: ctx.target_language || "zh",
    };
  }
  return { asrLanguage: "en", targetLanguage: "zh" };
}

export function translationFilePath(taskDir: string, lang: string): string {
  return join(taskDir, "translate", `translation.${lang}.json`);
}
export function readTranslationResult(ctx: TaskCtx) {
  if (!ctx.target_language) {
    throw new Error("ctx.target_language is required");
  }
  const filePath = translationFilePath(ctx.task.task_dir, ctx.target_language);
  if (!existsSync(filePath)) throw new Error(`translation file not found: ${filePath}`);
  return readJson<TranslateFile>(filePath, ctx);
}

export interface SrtJson {
  result: {
    text: string;
    segments: {
      text: string;
      start: number;
      end: number;
      confidence: number;
    }[];
  };
}
/**
 * - asr_ocr -> asr_ocr_fused.json
 * - asr_ocr + asr_ocr_fix?.llmFix -> asr_ocr_fused_llm_fix.json
 */
export function subtitleFilePath(ctx: TaskCtx): string {
  const src = ctx.input?.task?.subtitleSource ?? "asr";
  if (src === "ocr") {
    const fixFile = join(ctx.task.task_dir, "ocr_fix", "ocr_fix.json");
    return fixFile;
  }
  if (src === "asr_ocr") {
    const filename = ctx.input?.stages?.asr_ocr_fix?.llmFix
      ? "asr_ocr_fused_llm_fix.json"
      : "asr_ocr_fused.json";
    const fixFile = join(ctx.task.task_dir, "asr_ocr_fix", filename);
    return fixFile;
  }
  return join(ctx.task.task_dir, "asr_fix", "asr_fix.json");
}

export function split_audio_path(taskDir: string): string {
  return join(taskDir, "split_audio", "split_audio.json");
}
export function split_audio_timings_path(taskDir: string): string {
  return join(taskDir, "split_audio", "timings.json");
}
export function read_split_audio(ctx: TaskCtx) {
  const filepath = split_audio_path(ctx.task.task_dir);
  return readJson<SplitAudioFile>(filepath, ctx);
}
export function read_split_audio_timings(ctx: TaskCtx) {
  const filepath = split_audio_timings_path(ctx.task.task_dir);
  return readJson<SplitAudioTimingFile>(filepath, ctx);
}

/**
 * 目前修改此文件不会影响配音结果, 但重新运行 merge_video 时会改变生成的字幕时间位置
 */
export function timings_filepath(taskDir: string): string {
  return join(taskDir, "merge_audio", "timings.json");
}
export function read_timings(ctx: TaskCtx) {
  const filepath = timings_filepath(ctx.task.task_dir);
  if (!existsSync(filepath)) throw new Error(`timings file not found: ${filepath}`);
  return readJson<TimingsFile>(filepath, ctx);
}

export function tts_filepath(taskDir: string): string {
  return join(taskDir, "tts", "tts.json");
}

export function mixedVocalsPath(taskDir: string): string {
  return join(taskDir, "separate_after", "target_3_vocals_mixed.wav");
}

export function gatedVocalsPath(taskDir: string): string {
  return join(taskDir, "separate_after", "target_3_vocals_gated.wav");
}

export function dubbingPath(taskDir: string): string {
  return join(taskDir, "merge_audio", "audio_dubbing.wav");
}

export function finalVideoDir(
  pipeline: string,
  subtitleSource: SubtitleSource,
  noTranslate: boolean,
): string {
  const suffix = subtitleSource === "asr_ocr" ? "_asr_ocr" : subtitleSource === "ocr" ? "_ocr" : "";
  const ntlSuffix = noTranslate ? "_ntl" : "";
  const mode = pipeline === "subtitle" ? "subtitle" : "dub";
  return `${mode}${suffix}${ntlSuffix}`;
}

export function emitLog(taskDir: string, line: string) {
  const tid = getTaskId(taskDir);
  console.log(line);
  if (!tid) return;
  const ts = nowISO();
  const logPath = join(taskDir, `${tid}.log`);
  appendFileSync(logPath, `[${ts}] ${line}\n`);
}

export function ffmpeg(args: string[], timeout = 1_800_000) {
  const r = spawnSync(env.FFMPEG_PATH, ["-y", ...args], {
    stdio: ["pipe", "pipe", "pipe"],
    timeout,
  });
  if (r.error) {
    const e = r.error as NodeJS.ErrnoException;
    if (e.code === "ENOENT")
      throw new Error(
        `ffmpeg not found. Try: ${ffmpegInstallHint()}\nOr set FFMPEG_PATH in .env (currently "${env.FFMPEG_PATH}").\nDetails: ${e.message}`,
      );
    throw new Error(`ffmpeg failed: ${e.message}`);
  }
  if (r.status !== 0)
    throw new Error(`ffmpeg exit ${r.status}: ${r.stderr.toString().slice(-2000)}`);
}

function msDiff(a?: string | null, b?: string | null): number | null {
  if (!a || !b) return null;
  return Math.max(0, new Date(a).getTime() - new Date(b).getTime());
}

function fmtDuration(ms: number | null): string {
  if (ms == null) return "—";
  if (ms < 1000) return `${ms}ms`;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m${s % 60}s`;
}

function enrichStage(s: TaskStage) {
  return {
    ...s,
    progress: s.progress ?? 0,
    duration_ms: msDiff(s.completed_at, s.started_at),
  };
}

function buildSummary(stages: ReturnType<typeof enrichStage>[], task: Task): string {
  const done = stages.filter((s) => s.status === "success").length;
  const total = stages.length;
  const elapsedMs = msDiff(new Date().toISOString(), task.created_at) ?? 0;
  const elapsed = fmtDuration(elapsedMs);
  const stage = stages.find((s) => s.status === "running");
  const stageInfo = stage ? ` @ ${stage.label} (${stage.progress ?? 0}%)` : "";
  return task.status === "completed"
    ? `✅ completed in ${elapsed}`
    : `${task.status}${stageInfo} — ${done}/${total} stages done, elapsed ${elapsed}`;
}

export async function getStageStatuses(taskDir: string) {
  const { task, stages: rows = [] } = _readCtx(taskDir); // ensure ctx exists and is valid
  const pipeline = readCtx(taskDir)?.pipeline || "dub";

  const stageSpecs = getStages(pipeline);
  const stageMap = new Map(rows.map((r) => [r.name, r]));
  const stages = stageSpecs.map((s) =>
    enrichStage(
      stageMap.get(s) ?? {
        name: s,
        label: s,
        status: "pending",
        progress: 0,
        last_message: null,
        error_message: null,
        started_at: null,
        completed_at: null,
      },
    ),
  );

  return {
    taskId: task.id,
    url: task.url,
    title: task.title,
    status: task.status,
    current_stage: task.current_stage,
    created_at: task.created_at,
    started_at: task.started_at,
    completed_at: task.completed_at,
    task_dir: task.task_dir,
    final_video_path: task.final_video_path,
    error_message: task.error_message,
    stages,
    summary: buildSummary(stages, task),
  };
}
