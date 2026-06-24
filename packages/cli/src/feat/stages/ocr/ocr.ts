import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { OCREngine, type OCRRuntime } from "../../../ml/ocr/ocr.ts";
import { ensureDir, writeJson } from "../utils/fileOps.ts";
import { emitLog, ffmpeg, nowISO, srtTime, probeVideoResolution, videoSourcePath } from "../utils/utils.ts";

import { FrameResult, mergeFrames } from "../utils/ocrMerge.ts";
import { joinOcrLines, computeBoxYStats, computeSegmentAdjustments } from "./utils.ts";
import { Context, setStage } from "../../context/context.ts";

export async function stageOcr(ctx: Context) {
		const taskId = ctx.task.id;
		const sessionPath = ctx.task.session_path
	await setStage(sessionPath, "ocr", {
		last_message: "Extracting frames...",
		progress: 0,
	});

	const videoPath = videoSourcePath(sessionPath);
	if (!existsSync(videoPath)) {
		throw new Error(`OCR input not found: ${videoPath}`);
	}

	const ocrCfg = ctx.input?.stages?.ocr;
	const fps = ocrCfg?.fps ?? 2;
	const textScore = ocrCfg?.textScore ?? 0.45;
	const subtitleOnly = ocrCfg?.subtitleOnly ?? true;
	const runtime = (ocrCfg?.runtime ?? 'ort-cpp') as OCRRuntime;
	const device = (ocrCfg?.device ?? 'cpu') as 'cpu' | 'cuda' | 'directml' | 'coreml' | 'rocm' | 'mps';
	const cleanupFrames = ocrCfg?.cleanupFrames ?? false;
	const isoThresholdMs = ocrCfg?.isoThresholdMs ?? 1500;
	const adjustYWeight = ocrCfg?.adjustYWeight ?? 0.8;
	const adjustIsoWeight = ocrCfg?.adjustIsoWeight ?? 0.2;
	const adjustYFactor = ocrCfg?.adjustYFactor ?? 0.08;

	// 1. Extract frames
	const frameDir = join(sessionPath, "tmp", "ocr-frames");
	ensureDir(frameDir, ctx);
	emitLog(sessionPath, `[OCR] Extracting frames at ${fps}fps...`);

	const frProbe = spawnSync(
		"ffprobe",
		["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate", "-of", "csv=p=0", videoPath],
		{ timeout: 10_000, encoding: "utf-8" },
	);
	const frParts = (frProbe.stdout?.trim() || "30/1").split("/");
	const srcFps = parseInt(frParts[0]) / parseInt(frParts[1]);
	const step = Math.round(srcFps / fps);

	ffmpeg([
		"-y",
		"-i",
		videoPath,
		"-vf",
		`select='not(mod(n,${step}))'`,
		"-vsync",
		"vfr",
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
	await setStage(sessionPath, "ocr", {
		last_message: `OCR'ing ${frameFiles.length} frames (${runtime})...`,
	});

	const engine = new OCREngine(runtime, device);
	await engine.init();

	const linesArr = await engine.ocrFrames(frameDir, frameFiles, { textScore, subtitleOnly });
	const frameResults: FrameResult[] = [];
	for (let i = 0; i < frameFiles.length; i++) {
		const timestampMs = Math.round((i * step / srcFps) * 1000);
		try {
			const lines = linesArr[i];
			frameResults.push({ ...joinOcrLines(lines), timestamp: timestampMs });
		} catch {
			frameResults.push({ text: "", timestamp: timestampMs, confidence: 0 });
		}

		if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
			emitLog(sessionPath, `[OCR] ${i + 1}/${frameFiles.length} frames`);
		}
	}
	await engine.release();

	// 3. Merge into segments
	const segments = mergeFrames(frameResults);
	emitLog(
		sessionPath,
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
	const { height: videoHeight } = probeVideoResolution(videoPath);

	// 6. Write ocr.json (same format as asr_fix)
	const ocrDir = join(sessionPath, "ocr");
	ensureDir(ocrDir, ctx);
	const yStats = computeBoxYStats(frameResults);
	const adjustedSegments = computeSegmentAdjustments(segments, frameResults, yStats, videoHeight, isoThresholdMs, adjustYWeight, adjustIsoWeight, adjustYFactor);
	const segmentsOut = adjustedSegments.map((s) => ({ text: s.text, start: s.start, end: s.end, start_fmt: srtTime(s.start), end_fmt: srtTime(s.end), confidence: s.confidence, ...(s.box_y ? { box_y: s.box_y } : {}), ...(s.frameCount !== undefined ? { frameCount: s.frameCount } : {}), ...(s.adjustedConfidence !== undefined ? { adjustedConfidence: s.adjustedConfidence } : {}), ...(s.yPenalty !== undefined ? { yPenalty: s.yPenalty } : {}), ...(s.isoPenalty !== undefined ? { isoPenalty: s.isoPenalty } : {}) }));
	writeJson(
		join(ocrDir, "ocr.json"),
		{
			audio_info: { duration: audioDurMs || Math.round(videoDurationS * 1000) },
			result: { text, segments: segmentsOut },
			_engine: runtime,
			_device: device,
			_fps: fps,
			_textScore: textScore,
			_y_stats: yStats,
			_source: "ocr",
			_frames_raw: frameResults,
		},
		ctx,
	);

	emitLog(sessionPath, `[OCR] Written ${segments.length} segs to ocr.json`);

	// 7. Cleanup frames (optional)
	if (cleanupFrames) {
		rmSync(frameDir, { recursive: true, force: true });
		emitLog(sessionPath, `[OCR] Frames cleaned up`);
	} else {
		emitLog(sessionPath, `[OCR] Frames kept at ${frameDir}`);
	}

	await setStage(sessionPath, "ocr", {
		status: "succeeded",
		completed_at: nowISO(),
		progress: 100,
		last_message: `OCR'd ${frameFiles.length} frames → ${segments.length} segments`,
	});
}
