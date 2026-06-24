import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { OCREngine, type OCRRuntime } from '../../../ml/ocr/ocr.ts';
import { ensureDir, writeJson, readJson } from '../utils/fileOps.ts';
import { emitLog, nowISO, srtTime, videoSourcePath } from '../utils/utils.ts';
import { FrameResult, mergeFrames } from '../utils/ocrMerge.ts';
import { joinOcrLines, computeBoxYStats } from '../ocr/utils.ts';
import { Context, setStage } from '../../context/context.ts';

export async function stageAsrOcr(ctx: Context) {
	const sessionPath = ctx.task.session_path;
	await setStage(sessionPath, 'asr_ocr', {
		last_message: `OCR'ing frames...`,
		progress: 0,
	});

	const frameDir = join(sessionPath, 'asr_ocr_pre', 'frames');
	if (!existsSync(frameDir)) {
		throw new Error(`Frame directory not found: ${frameDir} — run asr_ocr_pre first`);
	}

	const asrOcrCfg = ctx.input?.stages?.asr_ocr;
	const textScore = asrOcrCfg?.textScore ?? 0.45;
	const subtitleOnly = asrOcrCfg?.subtitleOnly ?? true;
	const runtime = (asrOcrCfg?.runtime ?? 'ort-cpp') as OCRRuntime;
	const device = (asrOcrCfg?.device ?? 'cpu') as 'cpu' | 'cuda' | 'directml' | 'coreml' | 'rocm' | 'mps';
	const cleanupFrames = asrOcrCfg?.cleanupFrames ?? false;

	// OCR each frame
	const engine = new OCREngine(runtime, device);
	await engine.init();

	const frameFiles = readdirSync(frameDir).filter(f => f.endsWith('.jpg')).sort();
	const linesArr = await engine.ocrFrames(frameDir, frameFiles, { textScore, subtitleOnly });
	const frameResults: FrameResult[] = [];

	for (let i = 0; i < frameFiles.length; i++) {
		const tsMatch = frameFiles[i].match(/frame_(\d+)\.jpg/);
		const timestampMs = tsMatch ? parseInt(tsMatch[1]) : 0;
		const lines = linesArr[i];
		const r = joinOcrLines(lines);
		frameResults.push({ ...r, timestamp: timestampMs });

		if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
			emitLog(sessionPath, `[asr_ocr] ${i + 1}/${frameFiles.length} frames`);
		}
	}
	await engine.release();

	// Merge frames into OCR segments
	const ocrSegments = mergeFrames(frameResults);
	const ocrText = ocrSegments.map(s => s.text).join(' ');
	const audioDurMs = ocrSegments.length > 0 ? ocrSegments[ocrSegments.length - 1].end : 0;

	// probe video duration fallback
	const probe = spawnSync('ffprobe', [
		'-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', videoSourcePath(sessionPath),
	], { timeout: 15_000, encoding: 'utf-8' });
	const videoDurationS = parseFloat(probe.stdout?.trim() || '0');

	const asrOcrDir = resolve(sessionPath, 'asr_ocr');
	ensureDir(asrOcrDir, ctx);

	const yStats = computeBoxYStats(frameResults);

	const ocrSegmentsOut = ocrSegments.map(s => ({
		text: s.text,
		start: s.start,
		end: s.end,
		start_fmt: srtTime(s.start),
		end_fmt: srtTime(s.end),
		confidence: s.confidence,
		...(s.box_y ? { box_y: s.box_y } : {}),
		...(s.frameCount !== undefined ? { frameCount: s.frameCount } : {}),
	}));

	// Write ocr_frames.json — raw frame data for debugging/reproducibility
	writeJson(
		join(asrOcrDir, 'ocr_frames.json'),
		{
			_frames_raw: frameResults,
			_y_stats: yStats,
			_ocr_segments: ocrSegmentsOut,
		},
		ctx,
	);

	// Write asr_ocr.json — pure OCR-boundary segments (from mergeFrames)
	writeJson(
		join(asrOcrDir, 'asr_ocr.json'),
		{
			audio_info: { duration: audioDurMs || Math.round(videoDurationS * 1000) },
			_source: 'asr_ocr',
			_engine: runtime,
			_device: device,
			result: { text: ocrText, segments: ocrSegmentsOut },
		},
		ctx,
	);

	emitLog(sessionPath, `[asr_ocr] ${frameFiles.length} OCR frames → ${ocrSegments.length} OCR segments`);

	// Cleanup frames (optional)
	if (cleanupFrames) {
		rmSync(frameDir, { recursive: true, force: true });
		emitLog(sessionPath, `[asr_ocr] Frames cleaned up`);
	} else {
		emitLog(sessionPath, `[asr_ocr] Frames kept at ${frameDir}`);
	}

	await setStage(sessionPath, 'asr_ocr', {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
	});
}
