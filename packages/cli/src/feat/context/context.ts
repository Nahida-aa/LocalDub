import { RawConfigInput, TargetLang } from "../config/types";
import { readFileSync, writeFileSync } from 'node:fs';
import { delimiter, join } from 'node:path';
import { env, REPO_ROOT, WORKFOLDER } from '@repo/config';

import { fileLog, getLastSegment } from '../stages/utils/fileOps.ts';

export const getTaskId = (sessionPath: string) => getLastSegment(sessionPath)

export type VideoSource = 'youtube' | 'bilibili' | 'local' | 'remote'
export interface Task {
  id: string;
  source: VideoSource
  url: string;
  title?: string | null | undefined;
  status: string; // queued
  current_stage?: string | null | undefined;
  session_path: string
  final_video_path?: string | null | undefined;
  error_message?: string | null | undefined;
  created_at: string;
  started_at?: string | null | undefined;
  completed_at?: string | null | undefined;
}
export interface Context {
  task: Task;
  stages?: TaskStage[]
  pipeline: 'dub' | 'subtitle';
  lastRunPipeline?: 'dub' | 'subtitle'; // 用于 detect pipeline 切换
  input?: RawConfigInput
  runInfo?: {
		asr?: {
			engine: string; // 'whisper-pytorch' | 'faster-whisper'
			device: string;
			computeType?: string;
			gpuAttempted?: boolean;
			fallbackToCpu?: boolean;
		};
  }
	video_file_path?: string; // join(mediaDir, 'video_source.mp4')
  asr_language?: string; // ASR 自动检测的语言
	target_language?: TargetLang; // translate 阶段写入的目标语言: 如果 config 中没有指定 targetLang 则按照这个逻辑: 源语言: zh -> en, 否则 any -> zh
}


export interface TaskStage {
  completed_at?: string | null | undefined;
  error_message?: string | null | undefined;
  label: string;
  last_message?: string | null | undefined;
  name: string;
  progress?: number | null | undefined;
  started_at?: string | null | undefined;
  status: string;
}

export const ctxPath = (sessionPath: string) =>
	join(sessionPath, 'metadata', 'ctx.json');

/**
 * readFileSync, JSON.parse 都可以抛错, 如何处理交给使用者
 */
export const readCtx = (sessionPath: string) => {
	const path = ctxPath(sessionPath);
	const raw = _readCtx(sessionPath)
	console.log(`[File] read ${path}`);
	return raw 
};
export const _readCtx = (sessionPath: string) => {
	const path = ctxPath(sessionPath);
	const raw = JSON.parse(readFileSync(path, 'utf-8'));
	return raw as Context;
};
export const writeCtx = (ctx: Context) => {
	const path = ctxPath(ctx.task.session_path);
	const raw = JSON.stringify(ctx, null, 2);
	writeFileSync(path, raw);
	const lines = raw.split('\n').length;
	_writeCtx(ctx);
	console.log(`[${ctx.task.current_stage}] [File] write ${path} (${raw.length}B, ${lines} lines)`);
	return ctx;
};
export const _writeCtx = (ctx: Context) => {
	const path = ctxPath(ctx.task.session_path);
	const raw = JSON.stringify(ctx, null, 2);
	writeFileSync(path, raw);
	return ctx;
};
export const setCtx = (
	sessionPath: string,
	patch: Partial<Context>,
): void => {
	const existing = _readCtx(sessionPath) ?? ({} as Context);
	const ctx = _writeCtx( { ...existing, ...patch });
	console.log(`[${ctx.task.current_stage}] [File] set ${ctxPath(sessionPath)}:`, JSON.stringify(patch));
};
export const readTask = (sessionPath: string) => {
	const path = ctxPath(sessionPath);
	const raw = readFileSync(path, 'utf-8');
	const ctx = JSON.parse(raw) as Context;
	return ctx.task;
}
export const writeTask = ( task: Task) => {
		setCtx(task.session_path, { task });
}
export const setTask = (sessionPath: string, patch: Partial<Task>) => {
	const existing = readTask(sessionPath) ?? ({} as Task);
	const updated = { ...existing, ...patch };
	writeTask(updated);
}

export const listStage = (sessionPath: string) => _readCtx(sessionPath).stages ?? [];
const readStage = (sessionPath: string, stage: string) => {
	return listStage(sessionPath).find((s) => s.name === stage)
}
export const writeStages = (sessionPath: string, stages: TaskStage[]) => {
	const ctx = readCtx(sessionPath);
	writeCtx({...ctx, stages});
}
const writeStage = (sessionPath: string, stage: string, newStage: TaskStage) => {
	const ctx = _readCtx(sessionPath);
	const stages = ctx.stages ?? [];
	const idx = stages.findIndex((s) => s.name === stage);
	if (idx !== -1) {
		stages[idx] = newStage;
	} else {
		stages.push(newStage);
	}
	_writeCtx(ctx);
}
export const setStage = (sessionPath: string, stage: string, patch: Partial<TaskStage>) => {
	const existing = readStage(sessionPath, stage) ?? ({} as TaskStage);
	const updated = { ...existing, ...patch };
	writeStage(sessionPath, stage, updated);
}

export const readPipeline = (sessionPath: string) =>
	readCtx(sessionPath)?.pipeline || 'dub';
