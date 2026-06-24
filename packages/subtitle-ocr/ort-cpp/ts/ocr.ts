import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = resolve(fileURLToPath(import.meta.url), "..", "..");
const BINARY_PATH = resolve(__dirname, "build", "subtitle_ocr_ort_cpp");
const LD_PATH = resolve(__dirname, "build");

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

export function ocrFrame(
	framePath: string,
	opts?: { textScore?: number; subtitleOnly?: boolean },
): OCRLine[] {
	if (!existsSync(BINARY_PATH)) {
		throw new Error(
			`ocr_pipeline binary not found at ${BINARY_PATH}. Run 'npm run build:cpp' in packages/subtitle-ocr/`,
		);
	}

	const args: string[] = [framePath];
	if (opts?.textScore != null) args.push(String(opts.textScore));
	if (opts?.subtitleOnly) args.push("--subtitle-only");

	const r = spawnSync(BINARY_PATH, args, {
		timeout: 60_000,
		encoding: "utf-8",
		env: {
			...process.env,
			LD_LIBRARY_PATH: LD_PATH,
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
