import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { REPO_ROOT } from '../../feat/config/config.ts';
import { ocrFrameOpenCvCpp, ocrFramesOpenCvCpp } from './runtimes/ort-cpp.ts';
import { join } from 'node:path';
import { ocrFrameNode, createNodeSessions, releaseNodeSessions, type NodeSessions, type OCRDevice } from './runtimes/ort-node.ts';
import { ocrFramePy } from './runtimes/ort-py.ts';
import { runOcrFrame as runOcrFrameRust } from '../../../../subtitle-rust/ts/ocr.ts';

export type { NodeSessions } from './runtimes/ort-node.ts';

export type OCRRuntime = 'ort-cpp' | 'ort-node' | 'ort-py' | 'ort-rust';

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');

export function ocrBinaryPath(): string {
	const name = 'subtitle_ocr_ort_cpp' + (process.platform === 'win32' ? '.exe' : '');
	const candidates = [
		resolve(BUILD_DIR, 'Release', name),
		resolve(BUILD_DIR, name),
	];
	return candidates.find(c => existsSync(c)) || candidates[0];
}

export function ocrOrtDir(): string {
	if (process.platform === 'win32') {
		return resolve(REPO_ROOT, 'packages', 'tmp', 'onnxruntime-win-x64-1.26.0', 'onnxruntime-win-x64-1.26.0');
	}
	return '/tmp/onnxruntime-linux-x64-1.24.4';
}

/**
 * OCR engine that manages sessions for ort-node and dispatches per-frame calls.
 */
export class OCREngine {
	private runtime: OCRRuntime;
	private device: OCRDevice;
	private nodeSessions: NodeSessions | null = null;

	constructor(runtime: OCRRuntime, device: OCRDevice = 'cpu') {
		this.runtime = runtime;
		this.device = device;
	}

	async init(): Promise<void> {
		if (this.runtime === 'ort-node') {
			this.nodeSessions = await createNodeSessions(this.device);
		}
	}

	async ocrFrame(framePath: string, opts?: { textScore?: number; subtitleOnly?: boolean }): Promise<OCRLine[]> {
		switch (this.runtime) {
			case 'ort-cpp':
				return ocrFrameOpenCvCpp(framePath, { ...opts, device: this.device });
			case 'ort-node':
				if (!this.nodeSessions) throw new Error('Node sessions not initialized');
				return ocrFrameNode(framePath, this.nodeSessions, opts);
			case 'ort-py':
				return ocrFramePy(framePath, { ...opts, device: this.device });
			case 'ort-rust':
				return runOcrFrameRust(framePath, { ...opts, device: this.device }).segments;
			default:
				throw new Error(`Unknown OCR runtime: ${this.runtime}`);
		}
	}

	async ocrFrames(
		frameDir: string,
		frameFiles: string[],
		opts?: { textScore?: number; subtitleOnly?: boolean },
	): Promise<OCRLine[][]> {
		if (this.runtime === 'ort-cpp') {
			const resultMap = await ocrFramesOpenCvCpp(frameDir, { ...opts, device: this.device });
			return frameFiles.map((f) => resultMap.get(f) || []);
		}
		const results: OCRLine[][] = [];
		for (let i = 0; i < frameFiles.length; i++) {
			results.push(await this.ocrFrame(join(frameDir, frameFiles[i]), opts));
		}
		return results;
	}

	async release(): Promise<void> {
		if (this.runtime === 'ort-node' && this.nodeSessions) {
			await releaseNodeSessions(this.nodeSessions);
			this.nodeSessions = null;
		}
	}

	getRuntime(): OCRRuntime {
		return this.runtime;
	}
}

/**
 * Convenience function: creates a one-off engine for a single frame.
 * Prefer OCREngine for batch processing (avoids re-initialising node sessions.
 */
export async function ocrFrame(
	framePath: string,
	runtime: OCRRuntime = 'ort-cpp',
	opts?: { textScore?: number; subtitleOnly?: boolean; device?: OCRDevice },
): Promise<OCRLine[]> {
	const engine = new OCREngine(runtime, opts?.device ?? 'cpu');
	try {
		await engine.init();
		return await engine.ocrFrame(framePath, opts);
	} finally {
		await engine.release();
	}
}
