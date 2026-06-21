import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { ocrFrame, existsOcrBinary, ocrBinaryPath } from '../../ml/ocr/ocr.ts';
import { tryBuildOcr } from '../../ml/ocr/ocr-build.ts';
import { ensureDir, writeJson, readJson } from './utils/fileOps.ts';
import { emitLog, nowISO, srtTime } from './utils/utils.ts';
import { FrameResult, mergeFrames } from './utils/ocrMerge.ts';
import { Context, setStage } from '../context/context.ts';

export async function stageAsrOcr(ctx: Context) {
	const sessionPath = ctx.task.session_path;
	await setStage(sessionPath, 'asr_ocr', {
		last_message: 'ASR-guided OCR: extracting frames...',
		progress: 0,
	});

	const videoPath = join(sessionPath, 'media', 'video_source.mp4');
	if (!existsSync(videoPath)) {
		throw new Error(`Video not found: ${videoPath}`);
	}

	const asrFile = join(sessionPath, 'metadata', 'asr.json');
	if (!existsSync(asrFile)) {
		throw new Error(`asr.json not found: ${asrFile}`);
	}

	if (!existsOcrBinary()) {
		emitLog(sessionPath, `[ASR+OCR] OCR binary not found at ${ocrBinaryPath()}, attempting build...`);
		const built = await tryBuildOcr(sessionPath);
		if (!built) {
			throw new Error(`OCR binary not found at ${ocrBinaryPath()} and build failed.`);
		}
	}

	const ocrCfg = ctx.input?.stages?.ocr;
	const textScore = ocrCfg?.textScore ?? 0.45;
	const subtitleOnly = ocrCfg?.subtitleOnly ?? true;

	const asrData = await readJson(asrFile, ctx);
	const asrSegs: { text: string; start: number; end: number }[] = (asrData.result?.segments ?? []).map((s: any) => ({
		text: s.text,
		start: Math.round(s.start),
		end: Math.round(s.end),
	}));

	if (!asrSegs.length) {
		throw new Error('No ASR segments found');
	}

	// Generate frame timestamps: end2fps strategy
	// First segment: 10fps (100ms steps) for precise first-subtitle detection
	// Subsequent segments: 500ms steps backwards from end
	const allTimestamps = new Set<number>();
	for (let i = 0; i < asrSegs.length; i++) {
		const seg = asrSegs[i];
		if (i === 0) {
			for (let t = Math.round(seg.start); t <= Math.round(seg.end); t += 100) {
				allTimestamps.add(Math.round(t));
			}
		} else {
			for (let t = Math.round(seg.end); t >= seg.start; t -= 500) {
				allTimestamps.add(Math.round(t));
			}
		}
	}
	const sortedTs = [...allTimestamps].sort((a, b) => a - b);

	emitLog(sessionPath, `[ASR+OCR] ${asrSegs.length} ASR segs → ${sortedTs.length} frame positions`);

	// Extract frames
	const frameDir = join(sessionPath, 'tmp', 'asr-ocr-frames');
	ensureDir(frameDir, ctx);

	let ocrCount = 0;
	for (let i = 0; i < sortedTs.length; i++) {
		const ts = sortedTs[i];
		const framePath = join(frameDir, `frame_${ts.toString().padStart(7, '0')}.jpg`);
		const r = spawnSync('ffmpeg', [
			'-y', '-ss', String(ts / 1000), '-i', videoPath,
			'-frames:v', '1', '-qscale:v', '2', framePath,
		], { timeout: 15_000, encoding: 'utf-8' });
		if (r.status !== 0) continue;
		ocrCount++;

		if ((i + 1) % 50 === 0 || i === sortedTs.length - 1) {
			emitLog(sessionPath, `[ASR+OCR] Extracted ${i + 1}/${sortedTs.length} frames`);
		}
	}

	if (!ocrCount) {
		throw new Error('No frames extracted');
	}

	await setStage(sessionPath, 'asr_ocr', {
		last_message: `OCR'ing ${ocrCount} frames...`,
	});

	// OCR each frame
	const frameFiles = readdirSync(frameDir).filter(f => f.endsWith('.jpg')).sort();
	const frameResults: FrameResult[] = [];

	for (let i = 0; i < frameFiles.length; i++) {
		const framePath = join(frameDir, frameFiles[i]);
		const tsMatch = frameFiles[i].match(/frame_(\d+)\.jpg/);
		const timestampMs = tsMatch ? parseInt(tsMatch[1]) : 0;
		const lines = ocrFrame(framePath, { textScore, subtitleOnly });
		const best = lines.reduce(
			(a, b) => (a.confidence > b.confidence ? a : b),
			{ text: '', confidence: 0, box: [] },
		);
		frameResults.push({
			text: best.text,
			timestamp: timestampMs,
			confidence: best.confidence,
			box: best.box,
		});

		if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
			emitLog(sessionPath, `[ASR+OCR] ${i + 1}/${frameFiles.length} frames`);
		}
	}

	// Merge frames into OCR segments
	const ocrSegments = mergeFrames(frameResults);
	const ocrText = ocrSegments.map(s => s.text).join(' ');
	const audioDurMs = ocrSegments.length > 0 ? ocrSegments[ocrSegments.length - 1].end : 0;

	// probe video duration fallback
	const probe = spawnSync('ffprobe', [
		'-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', videoPath,
	], { timeout: 15_000, encoding: 'utf-8' });
	const videoDurationS = parseFloat(probe.stdout?.trim() || '0');

	const metadataDir = resolve(sessionPath, 'metadata');
	ensureDir(metadataDir, ctx);

	const ocrSegmentsOut = ocrSegments.map(s => ({
		text: s.text,
		start: s.start,
		end: s.end,
		start_fmt: srtTime(s.start),
		end_fmt: srtTime(s.end),
		...(s.box_y ? { box_y: s.box_y } : {}),
	}));

	// Write ocr_frames.json — raw frame data for debugging/reproducibility
	writeJson(
		join(metadataDir, 'ocr_frames.json'),
		{
			_frames_raw: frameResults,
			_ocr_segments: ocrSegmentsOut,
		},
		ctx,
	);

	// Write ocr.json — pure OCR-boundary segments (from mergeFrames)
	writeJson(
		join(metadataDir, 'ocr.json'),
		{
			audio_info: { duration: audioDurMs || Math.round(videoDurationS * 1000) },
			_source: 'asr_ocr',
			result: { text: ocrText, segments: ocrSegmentsOut },
		},
		ctx,
	);

	emitLog(sessionPath, `[ASR+OCR] ${ocrCount} OCR frames → ${ocrSegments.length} OCR segments`);

	// Cleanup
	rmSync(frameDir, { recursive: true, force: true });

	await setStage(sessionPath, 'asr_ocr', {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
	});
}
