import { readFileSync, writeFileSync, copyFileSync, rmSync, mkdirSync, existsSync, type WriteFileOptions } from 'node:fs';
import { emitLog } from './utils/utils.ts';
import { getTaskId } from './utils/context.ts';

function log(source: string, taskId: string | undefined, op: string, path: string, extra?: string) {
	emitLog(taskId || getTaskId(), `[${source}] [File] ${op} ${path}${extra ? ' ' + extra : ''}`);
}

export function readJson<T = any>(path: string, source: string, taskId?: string): T {
	log(source, taskId, 'read', path);
	return JSON.parse(readFileSync(path, 'utf-8'));
}

export function writeJson(path: string, data: any, source: string, taskId?: string) {
	const raw = JSON.stringify(data, null, 2);
	writeFileSync(path, raw);
	const lines = raw.split('\n').length;
	log(source, taskId, 'write', path, `(${raw.length}B, ${lines} lines)`);
}

export function writeFile(path: string, content: string | Buffer, source: string, taskId?: string) {
	writeFileSync(path, content);
	log(source, taskId, 'write', path, `(${Buffer.byteLength(content)}B)`);
}

export function copyFile(src: string, dst: string, source: string, taskId?: string) {
	copyFileSync(src, dst);
	log(source, taskId, 'copy', `${src} → ${dst}`);
}

export function removeFile(path: string, source: string, taskId?: string) {
	if (!existsSync(path)) return;
	rmSync(path);
	log(source, taskId, 'rm', path);
}

export function ensureDir(path: string, source: string, taskId?: string) {
	if (existsSync(path)) return;
	mkdirSync(path, { recursive: true });
	log(source, taskId, 'mkdir', path);
}

export { existsSync, readFileSync, rmSync, mkdirSync, writeFileSync, copyFileSync };
