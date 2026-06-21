import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { REPO_ROOT } from '../../../feat/config/config.ts';

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'subtitle-cpp', 'build');
const OCR_KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
const LIB_PATH_KEY = process.platform === 'win32' ? 'PATH' : 'LD_LIBRARY_PATH';

/**
 * Dynamically locate rapidocr model directory.
 * On Windows: .venv/Lib/site-packages/rapidocr_onnxruntime/models
 * On Linux:   .venv/lib/python3.x/site-packages/rapidocr_onnxruntime/models
 */
function findRapidOCRModelsDir(): string {
	const venvBase = resolve(REPO_ROOT, '.venv');
	const sitePackagesBase = resolve(venvBase, process.platform === 'win32' ? 'Lib' : 'lib', 'site-packages');
	if (!existsSync(sitePackagesBase)) {
		throw new Error(`site-packages not found: ${sitePackagesBase}`);
	}
	// Find pythonX.X directory on Linux
	let spDir = sitePackagesBase;
	if (process.platform !== 'win32') {
		const entries = readdirSync(spDir, { withFileTypes: true });
		const pyDir = entries.find(e => e.isDirectory() && e.name.startsWith('python'));
		if (pyDir) {
			spDir = join(spDir, pyDir.name);
		}
	}
	const modelsDir = join(spDir, 'rapidocr_onnxruntime', 'models');
	if (!existsSync(modelsDir)) {
		throw new Error(`rapidocr models not found: ${modelsDir}`);
	}
	return modelsDir;
}

const OCR_MODELS_DIR = findRapidOCRModelsDir();

function ocrBinaryPath(): string {
	const name = 'ocr_pipeline' + (process.platform === 'win32' ? '.exe' : '');
	const candidates = [
		resolve(BUILD_DIR, 'Release', name),
		resolve(BUILD_DIR, name),
	];
	return candidates.find(c => existsSync(c)) || candidates[0];
}

export function existsOcrBinary(): boolean {
	return existsSync(ocrBinaryPath());
}

export async function ocrFrameCpp(
	framePath: string,
	opts?: { textScore?: number; subtitleOnly?: boolean; device?: string },
): Promise<OCRLine[]> {
	const args: string[] = [framePath];
	if (opts?.textScore != null) args.push(String(opts.textScore));
	if (opts?.subtitleOnly) args.push('--subtitle-only');
	if (opts?.device && opts.device !== 'cpu') {
		args.push('--device', opts.device);
	}

	const r = spawnSync(ocrBinaryPath(), args, {
		timeout: 60_000,
		encoding: 'utf-8',
		env: {
			...process.env,
			[LIB_PATH_KEY]: `${BUILD_DIR}${process.platform === 'win32' ? ';' : ':'}${process.env[LIB_PATH_KEY] || ''}`,
		OCR_MODELS_DIR,
		OCR_KEYS_PATH,
		},
	});

	if (r.status !== 0) {
		throw new Error(`ocr_pipeline failed (exit ${r.status}): ${(r.stderr || '').slice(-300)}`);
	}

	const parsed = JSON.parse(r.stdout);

	const lines: OCRLine[] = [];
	for (const seg of parsed.segments || []) {
		lines.push({
			text: seg.text,
			confidence: seg.confidence,
			box: seg.box || [],
		});
	}
	if (lines.length === 0 && parsed.text) {
		lines.push({ text: parsed.text, confidence: 1, box: [] });
	}
	return lines;
}
