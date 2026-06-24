import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, cpSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, test, expect, beforeAll } from 'bun:test';

// ---- helpers (mirrors ocrEnv logic in cli/src/ml/ocr/runtimes/ort-opencv-cpp.ts) ----
const REPO_ROOT = resolve(import.meta.dir, '..', '..');
const BUILD_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'subtitle-opencv-cpp', 'build');
const BIN_NAME = 'ocr_pipeline_opencv' + (process.platform === 'win32' ? '.exe' : '');
const BIN_PATH = resolve(BUILD_DIR, BIN_NAME);
const MSYS2_BIN = resolve('C:\\', 'msys64', 'mingw64', 'bin');
const ORT_LIB_DIR = resolve(REPO_ROOT, 'packages', 'tmp', 'onnxruntime-win-x64-1.26.0',
	'onnxruntime-win-x64-1.26.0', 'lib');
const MODELS_DIR = resolve(REPO_ROOT, '.venv', 'Lib', 'site-packages', 'rapidocr_onnxruntime', 'models');
const KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
const SRC_FRAMES_DIR = resolve(REPO_ROOT, 'workfolder', 'local', '猫妖也疯狂', '8', 'asr_ocr_pre', 'frames');
const TMP_TEST_DIR = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', '.test-frames');

function ocrEnv(overrides?: Record<string, string>): Record<string, string> {
	const extra: string[] = [];
	if (process.platform === 'win32') {
		if (existsSync(MSYS2_BIN)) extra.push(MSYS2_BIN);
		if (existsSync(ORT_LIB_DIR)) extra.push(ORT_LIB_DIR);
	}
	const libPath = [...extra, BUILD_DIR, process.env.PATH || ''].filter(Boolean).join(';');
	return {
		...process.env as Record<string, string>,
		...overrides,
		PATH: libPath,
		OCR_MODELS_DIR: MODELS_DIR,
		OCR_KEYS_PATH: KEYS_PATH,
	};
}

function runBin(args: string[]) {
	return spawnSync(BIN_PATH, args, {
		encoding: 'utf-8',
		env: ocrEnv(),
	});
}

// Setup: copy first 5 existing frames to a temp dir for batch testing
beforeAll(async () => {
	if (!existsSync(TMP_TEST_DIR)) {
		mkdirSync(TMP_TEST_DIR, { recursive: true });
		const existing = readdirSync(SRC_FRAMES_DIR)
			.filter(f => /\.(jpg|jpeg|png|bmp)$/i.test(f))
			.sort()
			.slice(0, 3);
		for (const name of existing) {
			cpSync(resolve(SRC_FRAMES_DIR, name), resolve(TMP_TEST_DIR, name));
		}
	}
});

// ---- setup check ----
test('binary exists', () => {
	expect(existsSync(BIN_PATH)).toBe(true);
});

// ---- no arguments ----
test('no arguments prints usage and exits 1', () => {
	const r = runBin([]);
	expect(r.status).toBe(1);
	expect(r.stderr).toContain('Usage:');
});

// ---- invalid image path (single frame mode) ----
test('nonexistent image fails with non-zero exit', () => {
	const r = runBin(['/nonexistent/path.png']);
	expect(r.status).not.toBe(0);
});

// ---- valid single frame ----
describe('single frame', () => {
	const singleFrame = resolve(SRC_FRAMES_DIR, 'frame_0000000.jpg');

	test('frame exists', () => {
		expect(existsSync(singleFrame)).toBe(true);
	});

	test('returns valid JSON with correct structure', () => {
		const r = runBin([singleFrame, '0.5', '--no-nms']);
		expect(r.status).toBe(0);
		expect(r.stdout).toBeTruthy();

		const parsed = JSON.parse(r.stdout);
		expect(Array.isArray(parsed)).toBe(true);
		expect(parsed.length).toBeGreaterThanOrEqual(1);

		const item = parsed[0];
		expect(item).toHaveProperty('file');
		expect(item).toHaveProperty('segments');
		expect(Array.isArray(item.segments)).toBe(true);

		if (item.segments.length > 0) {
			const seg = item.segments[0];
			expect(seg).toHaveProperty('text');
			expect(typeof seg.text).toBe('string');
			expect(seg).toHaveProperty('confidence');
			expect(typeof seg.confidence).toBe('number');
			expect(seg.confidence).toBeGreaterThanOrEqual(0);
			expect(seg.confidence).toBeLessThanOrEqual(1);
			expect(seg).toHaveProperty('box');
			expect(Array.isArray(seg.box)).toBe(true);
		}
	});
});

// ---- frame directory (batch mode with 5 frames) ----
describe('frame directory', () => {
	test('test dir exists', () => {
		expect(existsSync(TMP_TEST_DIR)).toBe(true);
	});

	test('--dir processes frames and returns valid JSON', () => {
		const r = runBin(['--dir', TMP_TEST_DIR, '0.5', '--no-nms']);
		expect(r.status).toBe(0);
		expect(r.stdout).toBeTruthy();

		const parsed = JSON.parse(r.stdout);
		expect(Array.isArray(parsed)).toBe(true);
		expect(parsed.length).toBe(3);

		for (const item of parsed) {
			expect(item).toHaveProperty('file');
			expect(item).toHaveProperty('segments');
			expect(Array.isArray(item.segments)).toBe(true);

			for (const seg of item.segments) {
				expect(seg).toHaveProperty('text');
				expect(seg).toHaveProperty('confidence');
				expect(seg).toHaveProperty('box');
				expect(Array.isArray(seg.box)).toBe(true);
				expect(seg.box.length).toBe(4);
			}
		}
	});

	test('--subtitle-only runs without error', () => {
		const r = runBin(['--dir', TMP_TEST_DIR, '0.5', '--subtitle-only', '--no-nms']);
		expect(r.status).toBe(0);
		const parsed = JSON.parse(r.stdout);
		expect(Array.isArray(parsed)).toBe(true);
		expect(parsed.length).toBe(3);
	});
});

// ---- edge cases ----
test('directory without images returns empty JSON array', () => {
	const r = runBin(['--dir', REPO_ROOT, '0.5', '--no-nms']);
	expect(r.status).toBe(0);
	const parsed = JSON.parse(r.stdout);
	expect(Array.isArray(parsed)).toBe(true);
	expect(parsed.length).toBe(0);
});

test('missing OCR_MODELS_DIR fails', () => {
	const r = spawnSync(BIN_PATH, [], {
		encoding: 'utf-8',
		env: ocrEnv({ OCR_MODELS_DIR: '' }),
	});
	expect(r.status).toBe(1);
});
