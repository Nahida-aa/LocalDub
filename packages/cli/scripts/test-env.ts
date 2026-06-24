#!/usr/bin/env bun
/**
 * 测试 ocrFrameCpp 使用的环境变量
 */
import { spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
import { existsSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { to } from '@repo/shared/lib/utils/try';

const __filename = fileURLToPath(import.meta.url);
const __dirname = join(__filename, '..');

// 手动计算 REPO_ROOT - 从 packages/cli/scripts 向上三级
const REPO_ROOT = resolve(__dirname, '..', '..', '..');
console.log('[TEST] REPO_ROOT:', REPO_ROOT);
console.log('[TEST] process.platform:', process.platform);

// 复制 ort-cpp.ts 的逻辑来计算 OCR_MODELS_DIR
function findRapidOCRModelsDir(): string {
	const venvBase = resolve(REPO_ROOT, '.venv');
	const sitePackagesBase = resolve(venvBase, process.platform === 'win32' ? 'Lib' : 'lib', 'site-packages');
	console.log('[TEST] sitePackagesBase:', sitePackagesBase);
	if (!existsSync(sitePackagesBase)) {
		throw new Error(`site-packages not found: ${sitePackagesBase}`);
	}
	let spDir = sitePackagesBase;
	if (process.platform !== 'win32') {
		const entries = readdirSync(spDir, { withFileTypes: true });
		const pyDir = entries.find(e => e.isDirectory() && e.name.startsWith('python'));
		if (pyDir) {
			spDir = resolve(spDir, pyDir.name);
		}
	}
	const modelsDir = resolve(spDir, 'rapidocr_onnxruntime', 'models');
	console.log('[TEST] modelsDir:', modelsDir);
	if (!existsSync(modelsDir)) {
		throw new Error(`rapidocr models not found: ${modelsDir}`);
	}
	return modelsDir;
}

const main = () => {
	const OCR_MODELS_DIR = findRapidOCRModelsDir();
	console.log('[TEST] OCR_MODELS_DIR:', OCR_MODELS_DIR);

	// 检查 BUILD_DIR
	const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
	console.log('[TEST] BUILD_DIR:', BUILD_DIR);

	// 检查 OCR binary 路径
	const ocrBinary = resolve(BUILD_DIR, 'subtitle_ocr_ort_cpp.exe');
	console.log('[TEST] ocrBinary exists:', existsSync(ocrBinary));

	// 检查 OCR_KEYS_PATH
	const OCR_KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
	console.log('[TEST] OCR_KEYS_PATH:', OCR_KEYS_PATH);

	// 测试运行 OCR binary 看看环境变量
	console.log('\n[TEST] Running OCR binary with env...');
	const LIB_PATH_KEY = process.platform === 'win32' ? 'PATH' : 'LD_LIBRARY_PATH';
	const testEnv = {
		...process.env,
		[LIB_PATH_KEY]: `${BUILD_DIR}${process.platform === 'win32' ? ';' : ':'}${process.env[LIB_PATH_KEY] || ''}`,
		OCR_MODELS_DIR,
		OCR_KEYS_PATH,
	};
	console.log('[TEST] OCR_MODELS_DIR in env:', testEnv.OCR_MODELS_DIR);

	const result = spawnSync(ocrBinary, ['--help'], {
		timeout: 60_000,
		encoding: 'utf-8',
		env: testEnv,
	});

	console.log('[TEST] OCR exit code:', result.status);
	console.log('[TEST] OCR stderr:', result.stderr?.substring(0, 500));
}
const [_, err] = to(main)
if (err) {
		console.error('[ERROR]', err.message);
}

