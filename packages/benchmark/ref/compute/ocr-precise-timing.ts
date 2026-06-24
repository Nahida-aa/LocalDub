import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const REPO_ROOT = resolve(__dirname, '..', '..', '..', '..');
const CPP_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const DEFAULT_VIDEO = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'media', 'video_source.mp4');
const GT_PATH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'metadata', 'ocr_manual.json');

interface FrameResult {
	ms: number;
	text: string;
	confidence: number;
}

interface Segment {
	text: string;
	start: number;
	end: number;
	frames: number;
	confidence_avg: number;
}

interface Transition {
	ms: number;
	from: string;
	to: string;
}

interface Output {
	params: {
		start_ms: number;
		end_ms: number;
		fps: number;
		min_stable_ms: number;
		bridge_max_ms: number;
		video: string;
		subtitle_only: boolean;
	};
	timeline: FrameResult[];
	segments: Segment[];
	transitions: Transition[];
	gt_compare?: {
		matched: { text: string; gt_start: number; gt_end: number; ocr_start: number; ocr_end: number; start_diff: number; end_diff: number }[];
		in_gt_only: { text: string; start: number; end: number }[];
		in_ocr_only: { text: string; start: number; end: number }[];
	};
}

function parseArgs(): { start: number; end: number; fps: number; video: string; subtitleOnly: boolean; output: string | null } {
	const args = process.argv.slice(2);
	const get = (flag: string, def: string): string => {
		const i = args.indexOf(flag);
		return i >= 0 && i + 1 < args.length ? args[i + 1] : def;
	};
	return {
		start: parseInt(get('--start', '0'), 10),
		end: parseInt(get('--end', '10000'), 10),
		fps: parseInt(get('--fps', '100'), 10),
		video: get('--video', DEFAULT_VIDEO),
		subtitleOnly: args.includes('--subtitle-only') || !args.includes('--full-frame'),
		output: args.includes('--output') ? get('--output', '') : null,
	};
}

function ocrFrame(framePath: string): { text: string; confidence: number } {
	const r = spawnSync(CPP_BIN, [framePath, '0.5', '--subtitle-only'], {
		timeout: 60_000,
		encoding: 'utf-8',
		env: { ...process.env, LD_LIBRARY_PATH: CPP_LD_PATH },
	});
	if (r.status !== 0) return { text: '', confidence: 0 };
	try {
		const d = JSON.parse(r.stdout);
		const segs = d.segments || [];
		if (segs.length > 0) {
			const best = segs.reduce((a: any, b: any) => (a.confidence || 0) > (b.confidence || 0) ? a : b);
			return { text: best.text || '', confidence: best.confidence || 0 };
		}
		if (d.text) return { text: d.text, confidence: 1 };
		return { text: '', confidence: 0 };
	} catch {
		return { text: '', confidence: 0 };
	}
}

function loadGT(): Segment[] | null {
	try {
		const d = JSON.parse(readFileSync(GT_PATH, 'utf-8'));
		return (d.result?.segments || []).map((s: any) => ({
			text: s.text,
			start: s.start,
			end: s.end,
			frames: 0,
			confidence_avg: 0,
		}));
	} catch {
		return null;
	}
}

function textSimilarEnough(a: string, b: string): boolean {
	const shorter = a.length <= b.length ? a : b;
	const longer = a.length <= b.length ? b : a;
	return shorter.length === 0 || longer.includes(shorter) || [...shorter].filter(c => longer.includes(c)).length >= shorter.length * 0.5;
}

function main() {
	const params = parseArgs();
	console.log(`=== ocr-precise-timing ===`);
	console.log(`Range: ${params.start}ms - ${params.end}ms, fps=${params.fps}`);
	console.log(`Video: ${params.video}`);
	console.log();

	const totalFrames = Math.ceil((params.end - params.start) / 1000 * params.fps);
	const tmp = join(REPO_ROOT, 'packages', 'tmp', 'ocr-precise-timing');
	const framesDir = join(tmp, `frames_${params.start}_${params.end}_${params.fps}fps`);
	mkdirSync(framesDir, { recursive: true });

	console.log(`Extracting ${totalFrames} frames at ${params.fps}fps...`);
	const extractStart = Date.now();

	const segStart = params.start / 1000;
	const segEnd = params.end / 1000;
	const duration = segEnd - segStart;

	const ffmpegArgs = [
		'-y', '-i', params.video,
		'-ss', String(segStart),
		'-to', String(segEnd),
		'-vf', `fps=${params.fps}`,
		'-qscale:v', '2',
		join(framesDir, 'frame_%05d.jpg'),
	];

	const ffmpegR = spawnSync('ffmpeg', ffmpegArgs, { timeout: 120_000 });
	if (ffmpegR.status !== 0) {
		console.error(`ffmpeg failed: ${ffmpegR.stderr?.toString().slice(-200)}`);
		process.exit(1);
	}

	const frameFiles = readdirSync(framesDir).filter(f => f.endsWith('.jpg')).sort();
	console.log(`  ${frameFiles.length} frames extracted in ${((Date.now() - extractStart) / 1000).toFixed(1)}s`);
	console.log();

	// OCR each frame
	console.log(`Running OCR on ${frameFiles.length} frames...`);
	const ocrStart = Date.now();
	const rawFrames: FrameResult[] = [];

	for (let i = 0; i < frameFiles.length; i++) {
		const fp = join(framesDir, frameFiles[i]);
		const ms = Math.round(params.start + (i / params.fps) * 1000);
		const result = ocrFrame(fp);
		rawFrames.push({ ms, text: result.text, confidence: result.confidence });
	}

	const ocrElapsed = (Date.now() - ocrStart) / 1000;
	console.log(`  ${rawFrames.length} frames OCRed in ${ocrElapsed.toFixed(1)}s (${(ocrElapsed / rawFrames.length * 1000).toFixed(0)}ms/frame)`);
	console.log();

	// Group consecutive same-text raw frames (including empty)
	const minStableFrames = Math.max(1, Math.round(40 / 1000 * params.fps)); // 40ms minimum for noise filtering
	const groups: { text: string; startIdx: number; endIdx: number }[] = [];
	let grpStart = 0;
	for (let i = 1; i <= rawFrames.length; i++) {
		const curText = i < rawFrames.length ? rawFrames[i].text : null;
		const prevText = rawFrames[i - 1].text;
		if (curText !== prevText) {
			if (i - grpStart >= minStableFrames) {
				groups.push({ text: prevText, startIdx: grpStart, endIdx: i - 1 });
			}
			grpStart = i;
		}
	}

	// Merge adjacent groups with same text (separated by noise < 120ms)
	const merged: { text: string; startIdx: number; endIdx: number }[] = [];
	for (const g of groups) {
		const last = merged[merged.length - 1];
		const gapFrames = last ? g.startIdx - last.endIdx - 1 : Infinity;
		const gapMs = gapFrames / params.fps * 1000;
		if (last && last.text === g.text && gapMs < 120) {
			last.endIdx = g.endIdx;
		} else {
			merged.push({ ...g });
		}
	}

	// Build segments: bridge same-text groups separated by short noise
	const segments: Segment[] = [];
	const transitions: Transition[] = [];
	const activeSegments = new Map<string, { segIdx: number; endIdx: number; endMs: number }>();

	for (let i = 0; i < merged.length; i++) {
		const g = merged[i];
		if (!g.text) continue;
		const startMs = rawFrames[g.startIdx].ms;
		const endMs = rawFrames[g.endIdx].ms;

		const active = activeSegments.get(g.text);
		if (active) {
			const gapFrames = g.startIdx - active.endIdx - 1;
			const gapMs = gapFrames / params.fps * 1000;
			if (gapMs < 120) {
				segments[active.segIdx].end = i + 1 < merged.length ? rawFrames[merged[i + 1].startIdx].ms : params.end;
				active.endIdx = g.endIdx;
				active.endMs = endMs;
				continue;
			}
		}

		// New segment
		const segIdx = segments.length;
		// end = start of next group, or end of this group + 10ms
		const nextStart = i + 1 < merged.length ? rawFrames[merged[i + 1].startIdx].ms : params.end;
		const end = nextStart;
		if (end - startMs < 50) continue;

		const groupFrames = rawFrames.slice(g.startIdx, g.endIdx + 1).filter(f => f.text === g.text);
		const avgConf = groupFrames.length > 0
			? groupFrames.reduce((a, f) => a + f.confidence, 0) / groupFrames.length
			: 0;
		segments.push({
			text: g.text,
			start: startMs,
			end,
			frames: groupFrames.length,
			confidence_avg: parseFloat(avgConf.toFixed(3)),
		});
		activeSegments.set(g.text, { segIdx, endIdx: g.endIdx, endMs });

		const prevText = i > 0 ? merged[i - 1].text : '';
		if (g.text !== prevText) transitions.push({ ms: startMs, from: prevText, to: g.text });
	}

	// Compare with GT
	const gtSegs = loadGT();
	let gtCompare: Output['gt_compare'] = undefined;
	if (gtSegs) {
		const matched: Output['gt_compare']['matched'] = [];
		const inGtOnly: Output['gt_compare']['in_gt_only'] = [];
		const inOcrOnly: Output['gt_compare']['in_ocr_only'] = [];
		const gtUsed = new Set<number>();

		for (const os of segments) {
			if (!os.text) continue;
			let bestMatch: Segment | null = null;
			let bestOverlap = 0;
			for (let gi = 0; gi < gtSegs.length; gi++) {
				const gs = gtSegs[gi];
				if (!textSimilarEnough(os.text, gs.text)) continue;
				const overlap = Math.min(os.end, gs.end) - Math.max(os.start, gs.start);
				if (overlap > bestOverlap) {
					bestOverlap = overlap;
					bestMatch = gs;
				}
			}
			if (bestMatch) {
				const gi = gtSegs.indexOf(bestMatch);
				gtUsed.add(gi);
				matched.push({
					text: os.text,
					gt_start: bestMatch.start,
					gt_end: bestMatch.end,
					ocr_start: os.start,
					ocr_end: os.end,
					start_diff: os.start - bestMatch.start,
					end_diff: os.end - bestMatch.end,
				});
			} else {
				inOcrOnly.push({ text: os.text, start: os.start, end: os.end });
			}
		}

		for (let gi = 0; gi < gtSegs.length; gi++) {
			if (!gtUsed.has(gi)) {
				inGtOnly.push({ text: gtSegs[gi].text, start: gtSegs[gi].start, end: gtSegs[gi].end });
			}
		}

		gtCompare = { matched, in_gt_only: inGtOnly, in_ocr_only: inOcrOnly };
	}

	// Build output
	const output: Output = {
		params: {
			start_ms: params.start,
			end_ms: params.end,
			fps: params.fps,
			min_stable_ms: 40,
			bridge_max_ms: 120,
			video: params.video,
			subtitle_only: params.subtitleOnly,
		},
		timeline: rawFrames,
		segments,
		transitions,
		gt_compare: gtCompare,
	};

	// Write output
	const outputPath = params.output || join(tmp, `timing_${params.start}_${params.end}_${params.fps}fps.json`);
	writeFileSync(outputPath, JSON.stringify(output, null, 2));
	console.log(`Output: ${outputPath}`);

	// Print summary
	console.log();
	console.log(`=== Segments (${segments.length}) ===`);
	for (const s of segments) {
		const dur = s.end - s.start;
		console.log(`  [${s.start}-${s.end}] (${dur}ms, ${s.frames}frames) ${s.text}`);
	}

	if (gtCompare) {
		console.log();
		console.log(`=== GT Comparison ===`);
		console.log(`  Matched: ${gtCompare.matched.length}`);
		for (const m of gtCompare.matched) {
			const sd = m.start_diff > 0 ? `+${m.start_diff}` : `${m.start_diff}`;
			const ed = m.end_diff > 0 ? `+${m.end_diff}` : `${m.end_diff}`;
			console.log(`  ${m.text}: GT [${m.gt_start}-${m.gt_end}] → OCR [${m.ocr_start}-${m.ocr_end}] (Δstart=${sd}, Δend=${ed})`);
		}
		if (gtCompare.in_gt_only.length > 0) {
			console.log(`  GT only (${gtCompare.in_gt_only.length}):`);
			for (const g of gtCompare.in_gt_only) console.log(`    [${g.start}-${g.end}] ${g.text}`);
		}
		if (gtCompare.in_ocr_only.length > 0) {
			console.log(`  OCR only (${gtCompare.in_ocr_only.length}):`);
			for (const o of gtCompare.in_ocr_only) console.log(`    [${o.start}-${o.end}] ${o.text}`);
		}
	}
}

if (require.main === module) main();
