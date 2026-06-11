import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { ocrFrame } from "../../ml/ocr/ocr.ts";
import { REPO_ROOT, readConfig } from "../config/config.ts";
import { ensureDir, writeJson } from "./fileOps.ts";
import { emitLog, ffmpeg, nowISO, srtTime, updateStageDB } from "./utils.ts";

interface FrameResult {
	text: string;
	timestamp: number;
	confidence: number;
}

interface Segment {
	text: string;
	start: number;
	end: number;
}

function mergeFrames(frames: FrameResult[]): Segment[] {
	const segments: Segment[] = [];
	let currentText = "";
	let currentStart = 0;

	for (const f of frames) {
		if (!f.text) continue;
		if (f.text !== currentText) {
			if (currentText) {
				segments.push({
					text: currentText,
					start: currentStart,
					end: f.timestamp,
				});
			}
			currentText = f.text;
			currentStart = f.timestamp;
		}
	}
	if (currentText) {
		const lastTs =
			frames.length > 0 ? frames[frames.length - 1].timestamp : currentStart;
		segments.push({ text: currentText, start: currentStart, end: lastTs });
	}

	return segments.filter((s) => s.end - s.start >= 500);
}

export async function stageOcr(taskId: string, sessionPath: string) {
	await updateStageDB(taskId, "ocr", {
		last_message: "Extracting frames...",
		progress: 0,
	});

	const sessionAbsPath = resolve(REPO_ROOT, sessionPath);
	const videoPath = resolve(sessionAbsPath, "media", "video_source.mp4");
	if (!existsSync(videoPath)) {
		throw new Error(`OCR input not found: ${videoPath}`);
	}

	const ocrCfg = readConfig().stages?.ocr;
	const fps = ocrCfg?.fps ?? 1;
	const textScore = ocrCfg?.textScore ?? 0.3;

	// 1. Extract frames
	const frameDir = join(sessionAbsPath, "tmp", "ocr-frames");
	ensureDir(frameDir, "OCR");
	emitLog(taskId, `[OCR] Extracting frames at ${fps}fps...`);

	ffmpeg([
		"-y",
		"-i",
		videoPath,
		"-vf",
		`fps=${fps}`,
		"-qscale:v",
		"2",
		join(frameDir, "frame_%05d.jpg"),
	]);

	const frameFiles = readdirSync(frameDir)
		.filter((f) => f.endsWith(".jpg"))
		.sort();

	if (!frameFiles.length) {
		throw new Error(`OCR: no frames extracted from ${videoPath}`);
	}

	// 2. OCR each frame
	await updateStageDB(taskId, "ocr", {
		last_message: `OCR'ing ${frameFiles.length} frames...`,
	});

	const frameResults: FrameResult[] = [];
	for (let i = 0; i < frameFiles.length; i++) {
		const framePath = join(frameDir, frameFiles[i]);
		const timestampMs = Math.round((i / fps) * 1000);
		try {
			const lines = ocrFrame(framePath, { textScore });
			const best = lines.reduce(
				(a, b) => (a.confidence > b.confidence ? a : b),
				{ text: "", confidence: 0, box: [] },
			);
			frameResults.push({
				text: best.text,
				timestamp: timestampMs,
				confidence: best.confidence,
			});
		} catch {
			frameResults.push({ text: "", timestamp: timestampMs, confidence: 0 });
		}

		if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
			emitLog(taskId, `[OCR] ${i + 1}/${frameFiles.length} frames`);
		}
	}

	// 3. Merge into segments
	const segments = mergeFrames(frameResults);
	emitLog(
		taskId,
		`[OCR] ${frameFiles.length} frames → ${segments.length} segments`,
	);

	// 4. Build output text
	const text = segments.map((s) => s.text).join(" ");
	const audioDurMs = segments.length > 0 ? segments[segments.length - 1].end : 0;

	// 5. Probe video duration
	const probe = spawnSync(
		"ffprobe",
		[
			"-v",
			"error",
			"-show_entries",
			"format=duration",
			"-of",
			"csv=p=0",
			videoPath,
		],
		{ timeout: 15_000, encoding: "utf-8" },
	);
	const videoDurationS = parseFloat(probe.stdout?.trim() || "0");

	// 6. Write ocr.json (same format as asr_fix)
	const metadataDir = resolve(sessionAbsPath, "metadata");
	ensureDir(metadataDir, "OCR");
	const segmentsOut = segments.map((s) => ({ ...s, start_fmt: srtTime(s.start), end_fmt: srtTime(s.end) }));
	writeJson(
		join(metadataDir, "ocr.json"),
		{
			audio_info: { duration: audioDurMs || Math.round(videoDurationS * 1000) },
			result: { text, segments: segmentsOut },
			_engine: "rapidocr-onnxruntime",
			_fps: fps,
			_textScore: textScore,
			_source: "ocr",
		},
		"OCR",
	);

	emitLog(taskId, `[OCR] Written ${segments.length} segs to ocr.json`);

	// 7. Cleanup frames
	spawnSync("rm", ["-rf", frameDir]);

	await updateStageDB(taskId, "ocr", {
		status: "succeeded",
		completed_at: nowISO(),
		progress: 100,
		last_message: `OCR'd ${frameFiles.length} frames → ${segments.length} segments`,
	});
}
