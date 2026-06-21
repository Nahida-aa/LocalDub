import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { REPO_ROOT } from "../../feat/config/config.ts";

const BUILD_DIR = resolve(REPO_ROOT, "packages", "ocr-cpp", "build");
const LD_PATH = BUILD_DIR;

function ocrBinaryPathInner(): string {
	const name = "ocr_pipeline" + (process.platform === "win32" ? ".exe" : "");
	const candidates = [
		resolve(BUILD_DIR, "Release", name),
		resolve(BUILD_DIR, name),
	];
	return candidates.find(c => existsSync(c)) || candidates[0];
}

const LIB_PATH_KEY = process.platform === "win32" ? "PATH" : "LD_LIBRARY_PATH";

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

const OCR_KEYS_PATH = resolve(REPO_ROOT, "packages", "benchmark", "ocr", "compute", "ppocr_keys.json");

interface OcrEnv {
	OCR_MODELS_DIR: string;
	OCR_KEYS_PATH: string;
}

let _ocrEnv: OcrEnv | null = null;

function resolveOcrEnv(): OcrEnv {
	if (_ocrEnv) return _ocrEnv;

	const keysPath = OCR_KEYS_PATH;
	if (!existsSync(keysPath)) {
		throw new Error(`OCR keys not found at ${keysPath}`);
	}

	const pyBin = process.env.VIRTUAL_ENV
		? join(process.env.VIRTUAL_ENV, "Scripts", "python.exe")
		: "python";
	const r = spawnSync(pyBin, ["-c",
		"import rapidocr_onnxruntime, os; print(os.path.join(os.path.dirname(rapidocr_onnxruntime.__file__), 'models'))",
	], { timeout: 15_000, encoding: "utf-8" });
	if (r.status !== 0) {
		throw new Error(
			`Failed to resolve OCR model path (is rapidocr-onnxruntime installed?): ${(r.stderr || "").slice(-200)}`,
		);
	}
	const modelsDir = r.stdout.trim();
	if (!existsSync(modelsDir)) {
		throw new Error(`OCR models dir resolved but not found: ${modelsDir}`);
	}

	_ocrEnv = { OCR_MODELS_DIR: modelsDir, OCR_KEYS_PATH: keysPath };
	return _ocrEnv;
}

export function ocrBinaryPath(): string {
	return ocrBinaryPathInner();
}

export function existsOcrBinary(): boolean {
	const name = "ocr_pipeline" + (process.platform === "win32" ? ".exe" : "");
	return existsSync(resolve(BUILD_DIR, "Release", name)) || existsSync(resolve(BUILD_DIR, name));
}

export function ocrOrtDir(): string {
	if (process.platform === "win32") {
		return resolve(REPO_ROOT, "packages", "tmp", "onnxruntime-win-x64-1.26.0", "onnxruntime-win-x64-1.26.0");
	}
	return "/tmp/onnxruntime-linux-x64-1.24.4";
}

export function ocrFrame(
	framePath: string,
	opts?: { textScore?: number; subtitleOnly?: boolean },
): OCRLine[] {
	if (!existsOcrBinary()) {
		throw new Error(
			`ocr_pipeline binary not found at ${ocrBinaryPath()}. Run 'npm run build' in packages/ocr-cpp/`,
		);
	}

	const env = resolveOcrEnv();

	const args: string[] = [framePath];
	if (opts?.textScore != null) args.push(String(opts.textScore));
	if (opts?.subtitleOnly) args.push("--subtitle-only");

	const r = spawnSync(ocrBinaryPath(), args, {
		timeout: 60_000,
		encoding: "utf-8",
		env: {
			...process.env,
			...env,
			[LIB_PATH_KEY]: `${LD_PATH}${process.platform === "win32" ? ";" : ":"}${process.env[LIB_PATH_KEY] || ""}`,
		},
	});

	if (r.status !== 0) {
		throw new Error(
			`ocr_pipeline failed (exit ${r.status}): ${(r.stderr || "").slice(-300)}`,
		);
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
