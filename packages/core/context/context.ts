import { CliInput } from "../input/types";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { getLastSegment } from "../utils/fileOps.ts";
import { TargetLang } from "../tasks/args.ts";
import { TaskStage } from "./types.ts";

export const getTaskId = (taskDir: string) => getLastSegment(taskDir);

export type VideoSource = "youtube" | "bilibili" | "local" | "remote" | "unknown";

export interface TaskBrief {
  id: string;
  title?: string | null;
  status: string; // queued
  current_stage?: string | null;
  created_at: string;
  started_at?: string | null;
  completed_at?: string | null;
  error_message?: string | null;
}

export interface Task extends TaskBrief {
  source: VideoSource;
  url: string;
  task_dir: string;
  final_video_path?: string | null | undefined;
}
export interface FrameRate {
  numerator: number; // 分子
  denominator: number; // 分母
}
export interface TaskCtx {
  task: Task;
  stages?: TaskStage[];
  pipeline: "dub" | "subtitle";
  lastRunPipeline?: "dub" | "subtitle"; // 用于 detect pipeline 切换
  input: CliInput;
  frame_rate: FrameRate;
  runInfo?: {
    asr?: {
      engine: string; // 'whisper-pytorch' | 'faster-whisper'
      device: string;
      computeType?: string;
      gpuAttempted?: boolean;
      fallbackToCpu?: boolean;
    };
  };
  video_source_path?: string; //
  audioSourcePath?: string; //
  asr_language?: string; // ASR 自动检测的语言
  target_language?: TargetLang; // translate 阶段写入的目标语言: 如果 config 中没有指定 targetLang 则按照这个逻辑: 源语言: zh -> en, 否则 any -> zh
}

export const ctxPath = (taskDir: string) => join(taskDir, "ctx.json");

/**
 * readFileSync, JSON.parse 都可以抛错, 如何处理交给使用者
 */
export const readCtx = (taskDir: string) => {
  const path = ctxPath(taskDir);
  const raw = _readCtx(taskDir);
  console.log(`[File] read ${path}`);
  return raw;
};
export const _readCtx = (taskDir: string) => {
  const path = ctxPath(taskDir);
  const raw = JSON.parse(readFileSync(path, "utf-8"));
  return raw as TaskCtx;
};
export const writeCtx = (ctx: TaskCtx) => {
  const path = ctxPath(ctx.task.task_dir);
  const raw = JSON.stringify(ctx, null, 2);
  writeFileSync(path, raw);
  const lines = raw.split("\n").length;
  _writeCtx(ctx);
  console.log(`[${ctx.task.current_stage}] [File] write ${path} (${raw.length}B, ${lines} lines)`);
  return ctx;
};
export const _writeCtx = (ctx: TaskCtx) => {
  const path = ctxPath(ctx.task.task_dir);
  const raw = JSON.stringify(ctx, null, 2);
  writeFileSync(path, raw);
  return ctx;
};
export const _setCtx = (taskDir: string, patch: Partial<TaskCtx>) => {
  const existing = _readCtx(taskDir) ?? ({} as TaskCtx);
  const ctx = _writeCtx({ ...existing, ...patch });
  return ctx;
};
export const setCtx = (taskDir: string, patch: Partial<TaskCtx>) => {
  const ctx = _setCtx(taskDir, patch);
  console.log(`[${ctx.task.current_stage}] setCtx ${ctxPath(taskDir)}:`, JSON.stringify(patch));
  return ctx;
};
export const readTask = (taskDir: string) => {
  const path = ctxPath(taskDir);
  const raw = readFileSync(path, "utf-8");
  const ctx = JSON.parse(raw) as TaskCtx;
  return ctx.task;
};
export const _writeTask = (task: Task) => {
  _setCtx(task.task_dir, { task });
};
export const writeTask = (task: Task) => {
  _setCtx(task.task_dir, { task });
  console.log(`[${task.current_stage}] setTask ${ctxPath(task.task_dir)}:`, JSON.stringify(task));
};
export const setTask = (taskDir: string, patch: Partial<Task>) => {
  // If marking success, clear any previous error_message to avoid stale failure state
  if (patch.status === "success") {
    patch.error_message = null;
  }
  const existing = readTask(taskDir) ?? ({} as Task);
  const updated = { ...existing, ...patch };
  _writeTask(updated);
  console.log(`[${updated.current_stage}] setTask:`, JSON.stringify(patch));
};

export const listStage = (taskDir: string) => _readCtx(taskDir).stages ?? [];
const readStage = (taskDir: string, stage: string) => {
  return listStage(taskDir).find((s) => s.name === stage);
};
export const writeStages = (taskDir: string, stages: TaskStage[]) => {
  const ctx = readCtx(taskDir);
  writeCtx({ ...ctx, stages });
};
const writeStage = (taskDir: string, stage: string, newStage: TaskStage) => {
  const ctx = _readCtx(taskDir);
  const stages = ctx.stages ?? [];
  const idx = stages.findIndex((s) => s.name === stage);
  if (idx !== -1) {
    stages[idx] = newStage;
  } else {
    stages.push(newStage);
  }
  _writeCtx(ctx);
};
export const setStage = (taskDir: string, stage: string, patch: Partial<TaskStage>) => {
  _readCtx(taskDir);
  const existing = readStage(taskDir, stage) ?? ({} as TaskStage);
  const updated = { ...existing, ...patch };
  if (updated.status === "success") updated.error_message = null as any;
  writeStage(taskDir, stage, updated);
  console.log(
    `[${_readCtx(taskDir).task.current_stage}] setStage: ${stage}`,
    JSON.stringify(patch),
  );
};

export const readPipeline = (taskDir: string) => readCtx(taskDir)?.pipeline || "dub";
