import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { getRapidOCRModelsDir } from '../rapidocr-models.ts';
import { REPO_ROOT } from '../../../feat/config/config.ts';

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const OCR_KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
function getLibPathKey(): string {
	if (process.platform !== 'win32') return 'LD_LIBRARY_PATH';
	const existing = Object.keys(process.env).find(k => k.toLowerCase() === 'path');
	return existing || 'PATH';
}
const LIB_PATH_KEY = getLibPathKey();

function ocrOpenCvCppBinaryPath(): string {
	const name = 'subtitle_ocr_ort_cpp' + (process.platform === 'win32' ? '.exe' : '');
	const candidates = [
		resolve(BUILD_DIR, 'Release', name),
		resolve(BUILD_DIR, name),
	];
	return candidates.find(c => existsSync(c)) || candidates[0];
}

export function existsOcrOpenCvCppBinary(): boolean {
	return existsSync(ocrOpenCvCppBinaryPath());
}

export async function ocrFrameOpenCvCpp(
	framePath: string,
	opts?: { textScore?: number; subtitleOnly?: boolean; device?: string },
): Promise<OCRLine[]> {
	const r = spawnSync(ocrOpenCvCppBinaryPath(), [
		framePath,
		...(opts?.textScore != null ? [String(opts.textScore)] : []),
		...(opts?.subtitleOnly ? ['--subtitle-only'] : []),
		...(opts?.device && opts.device !== 'cpu' ? ['--device', opts.device] : []),
	], {
		timeout: 60_000,
		encoding: 'utf-8',
		env: ocrEnv(),
	});

	if (r.status !== 0) {
		throw new Error(`subtitle_ocr_ort_cpp failed (exit ${r.status}): ${(r.stderr || '').slice(-300)}`);
	}

	const parsed = parseBatchOutput(r.stdout);
	const filename = basename(framePath);
	return parsed.get(filename) || [];
}

export async function ocrFramesOpenCvCpp(
	frameDir: string,
	opts?: { textScore?: number; subtitleOnly?: boolean; device?: string },
): Promise<Map<string, OCRLine[]>> {
	const binPath = ocrOpenCvCppBinaryPath();
	const args = [
		'--dir', frameDir,
		...(opts?.textScore != null ? [String(opts.textScore)] : []),
		...(opts?.subtitleOnly ? ['--subtitle-only'] : []),
		...(opts?.device && opts.device !== 'cpu' ? ['--device', opts.device] : []),
	];
	const env = ocrEnv();
	const r = spawnSync(binPath, args, {
		encoding: 'utf-8',
		env,
	});

	if (r.status !== 0) {
		const stderr = (r.stderr || '').slice(-500);
		const stdout = (r.stdout || '').slice(-500);
		const diag = [
			`exit=${r.status} signal=${r.signal}`,
			`bin=${binPath}`,
			`frameDir=${frameDir}`,
			`msys2Bin exists=${existsSync(resolve('C:\\', 'msys64', 'mingw64', 'bin'))}`,
			`ortLib exists=${existsSync(ORT_LIB_DIR)}`,
			`buildDir exists=${existsSync(BUILD_DIR)}`,
			stderr && `stderr:\n${stderr}`,
			stdout && `stdout:\n${stdout}`,
		].filter(Boolean).join('\n');
		throw new Error(`subtitle_ocr_ort_cpp --dir failed\n${diag}`);
	}

	return parseBatchOutput(r.stdout);
}

/** Directory containing onnxruntime.dll + MinGW runtime DLLs (libstdc++-6.dll, etc.) */
const ORT_LIB_DIR = resolve(REPO_ROOT, 'packages', 'tmp', 'onnxruntime-win-x64-1.26.0',
	'onnxruntime-win-x64-1.26.0', 'lib');

function ocrEnv(): Record<string, string | undefined> {
	const msys2Bin = resolve('C:\\', 'msys64', 'mingw64', 'bin');
	const extra: string[] = [];
	if (process.platform === 'win32') {
		if (existsSync(msys2Bin)) extra.push(msys2Bin);
		if (existsSync(ORT_LIB_DIR)) extra.push(ORT_LIB_DIR);
	}
	const libPath = [...extra, BUILD_DIR, process.env[LIB_PATH_KEY] || ''].filter(Boolean).join(';');
	return {
		...process.env,
		[LIB_PATH_KEY]: libPath,
		OCR_MODELS_DIR: getRapidOCRModelsDir(),
		OCR_KEYS_PATH,
	};
}

function parseBatchOutput(stdout: string): Map<string, OCRLine[]> {
	const items: any[] = JSON.parse(stdout);
	const result = new Map<string, OCRLine[]>();
	for (const item of items) {
		const lines: OCRLine[] = [];
		for (const seg of item.segments || []) {
			lines.push({
				text: seg.text,
				confidence: seg.confidence,
				box: seg.box || [],
			});
		}
		if (lines.length === 0 && item.text) {
			lines.push({ text: item.text, confidence: 1, box: [] });
		}
		result.set(item.file || '', lines);
	}
	return result;
}


