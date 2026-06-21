import { existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT } from '../../feat/config/config.ts';
import { emitLog } from '../../feat/stages/utils/utils.ts';
import { cmakeBin } from '../../feat/stages/separate/separate-build.ts';
import { ocrBinaryPath, ocrOrtDir } from './ocr.ts';

const ORT_TMP_DIR = resolve(REPO_ROOT, 'packages', 'tmp');
const SRC_DIR = resolve(REPO_ROOT, 'packages', 'ocr-cpp');
const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'ocr-cpp', 'build');

function ortDirExists(): boolean {
	const dir = ocrOrtDir();
	return existsSync(join(dir, 'include', 'onnxruntime_c_api.h'));
}

function ensureOrt(sessionPath: string): boolean {
	if (ortDirExists()) return true;

	if (process.platform !== 'win32') {
		emitLog(sessionPath, '[OCR] ONNX Runtime not found. Install manually:\n'
			+ '  Download and extract to /tmp/onnxruntime-linux-x64-<version>');
		return false;
	}

	emitLog(sessionPath, '[OCR] Downloading ONNX Runtime 1.26.0 for Windows (72 MB)...');
	const zipPath = join(ORT_TMP_DIR, 'onnxruntime-win-x64-1.26.0.zip');
	const url = 'https://github.com/microsoft/onnxruntime/releases/download/v1.26.0/onnxruntime-win-x64-1.26.0.zip';

	const dl = spawnSync('curl.exe', ['-#L', '-o', zipPath, url], { timeout: 300_000 });
	if (dl.status !== 0) {
		emitLog(sessionPath, '[OCR] Failed to download ONNX Runtime.\n'
			+ `  Download manually: ${url}\n`
			+ `  Extract to: ${ocrOrtDir()}`);
		return false;
	}

	emitLog(sessionPath, '[OCR] Extracting...');
	const extract = spawnSync('tar', ['-xf', zipPath, '-C', ORT_TMP_DIR], { timeout: 60_000 });
	if (extract.status !== 0) {
		emitLog(sessionPath, '[OCR] Failed to extract ONNX Runtime.\n'
			+ `  Extract ${zipPath} to ${ocrOrtDir()} manually.`);
		return false;
	}

	if (!ortDirExists()) {
		emitLog(sessionPath, '[OCR] Extracted but ORT headers not found.\n'
			+ `  Expected at: ${join(ocrOrtDir(), 'include', 'onnxruntime_c_api.h')}`);
		return false;
	}

	emitLog(sessionPath, '[OCR] ONNX Runtime ready');
	return true;
}

export async function tryBuildOcr(sessionPath: string): Promise<boolean> {
	const log = (msg: string) => emitLog(sessionPath, msg);

	if (existsSync(ocrBinaryPath())) {
		log('[OCR] Binary already exists, skipping build');
		return true;
	}

	log('[OCR] Checking build prerequisites...');

	const cmakePath = cmakeBin();
	const cmakeCheck = spawnSync(cmakePath, ['--version'], { timeout: 5000 });
	if (cmakeCheck.status !== 0) {
		log('[OCR] cmake not found — install CMake first');
		return false;
	}

	if (!ensureOrt(sessionPath)) {
		log('[OCR] ONNX Runtime not available, cannot build');
		return false;
	}

	mkdirSync(BUILD_DIR, { recursive: true });

	const ortDir = ocrOrtDir();
	log(`[OCR] Running cmake configure (ORT=${ortDir})...`);
	const configure = spawnSync(cmakePath, [
		'-DORT_DIR=' + ortDir,
		'-DCMAKE_BUILD_TYPE=Release',
		'-S', SRC_DIR,
		'-B', BUILD_DIR,
	], { timeout: 60_000 });
	if (configure.status !== 0) {
		const stderr = configure.stderr?.toString() || '(no stderr)';
		log(`[OCR] cmake configure failed:\n${stderr.slice(0, 500)}`);
		return false;
	}

	log('[OCR] Building (may take a moment)...');
	const build = spawnSync(cmakePath, ['--build', BUILD_DIR, '--config', 'Release', '-j', '4'], {
		timeout: 300_000,
	});
	if (build.status !== 0) {
		const stderr = build.stderr?.toString() || '(no stderr)';
		log(`[OCR] Build failed:\n${stderr.slice(0, 1000)}`);
		return false;
	}

	log(`[OCR] Built successfully: ${ocrBinaryPath()}`);
	return true;
}
