import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const REPO_ROOT = resolve(__dirname, '..', '..', '..', '..');
const WER_PY = resolve(REPO_ROOT, 'packages', 'benchmark', 'separate', 'compute', 'wer.py');
const CPP_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'metadata', 'ocr_manual.json');
const VIDEO_PATH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'media', 'video_source.mp4');
const ASR_SOURCE = process.env.ASR_SOURCE || 'ggml-s1-sidechain';
const ASR_PARAM = process.env.ASR_PARAM || 'baseline';
const ASR_RAW = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr', 'whisper', 'results', ASR_SOURCE, ASR_PARAM, 'metadata', 'whisper_raw.json');
const RESULTS_BASE = resolve(__dirname, '..', 'results');
const TMP_DIR = resolve(REPO_ROOT, 'packages', 'tmp', 'asr-guided-ocr');

interface Segment {
	text: string;
	start: number;
	end: number;
}

interface ASRSeg extends Segment {
	text: string;
}

interface OCRLine {
	text: string;
	confidence: number;
}

function loadGT(): Segment[] {
	return JSON.parse(readFileSync(GROUND_TRUTH, 'utf-8')).result.segments;
}

function loadASR(): ASRSeg[] {
	const raw = JSON.parse(readFileSync(ASR_RAW, 'utf-8'));
	return raw.transcription.map((s: any) => ({
		text: s.text,
		start: s.offsets.from,
		end: s.offsets.to,
	}));
}

function ocrFrame(framePath: string): OCRLine[] {
	const r = spawnSync(CPP_BIN, [framePath], {
		timeout: 60_000,
		encoding: 'utf-8',
		env: { ...process.env, LD_LIBRARY_PATH: CPP_LD_PATH },
	});
	if (r.status !== 0) return [];
	try {
		const parsed = JSON.parse(r.stdout);
		const lines: OCRLine[] = [];
		for (const seg of parsed.segments || []) {
			lines.push({ text: seg.text, confidence: seg.confidence });
		}
		if (lines.length === 0 && parsed.text) {
			lines.push({ text: parsed.text, confidence: 1 });
		}
		return lines;
	} catch { return []; }
}

function mergeSegments(segs: Segment[], sameTextGapMs: number = 1000): Segment[] {
	const merged: Segment[] = [];
	for (const s of segs) {
		if (!s.text) continue;
		const last = merged[merged.length - 1];
		if (last && last.text === s.text && s.start - last.end <= sameTextGapMs) {
			last.end = Math.max(last.end, s.end);
		} else {
			merged.push({ ...s });
		}
	}
	return merged;
}

function calcCERDetail(hypText: string): any {
	const refText = loadGT().map((s) => s.text).join('');
	const r = spawnSync('python3', [WER_PY, '--text', refText, hypText], {
		timeout: 15_000,
		encoding: 'utf-8',
	});
	if (r.status !== 0) throw new Error(`wer.py failed: ${r.stderr}`);
	return JSON.parse(r.stdout);
}

function segmentsToText(segs: Segment[]): string {
	return segs.map((s) => s.text).join('');
}

function formatTime(ms: number): string {
	const h = Math.floor(ms / 3600000);
	const m = Math.floor((ms % 3600000) / 60000);
	const s = Math.floor((ms % 60000) / 1000);
	const ml = ms % 1000;
	return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')},${ml.toString().padStart(3, '0')}`;
}

function extractFrame(videoPath: string, timeMs: number, outPath: string): boolean {
	const r = spawnSync('ffmpeg', [
		'-y', '-ss', String(timeMs / 1000),
		'-i', videoPath,
		'-frames:v', '1',
		'-qscale:v', '2',
		outPath,
	], { timeout: 10_000, encoding: 'utf-8' });
	return r.status === 0;
}

function run(label: string, segs: Segment[], desc: string, gtSegs: Segment[], asrSrc: ASRSeg[], ocrCallCount?: number, elapsedSec?: number) {
	const text = segmentsToText(segs);
	const cerDetail = calcCERDetail(text);

	const resultDir = join(RESULTS_BASE, label);
	mkdirSync(resultDir, { recursive: true });
	mkdirSync(join(resultDir, 'metadata'), { recursive: true });

	// Coverage: which GT segments are covered by hyp
	let coveredSegs = 0;
	let coveredChars = 0;
	for (const g of gtSegs) {
		if (!g.text) continue;
		const covered = segs.some((h) =>
			h.start <= g.end + 1000 && h.end >= g.start - 1000 &&
			([...g.text].some((c) => h.text.includes(c)) || g.text.length < 3)
		);
		if (covered) {
			coveredSegs++;
			coveredChars += g.text.length;
		}
	}
	const totalSegs = gtSegs.filter((s) => s.text).length;
	const totalChars = gtSegs.reduce((a, s) => a + s.text.length, 0);

	const summary = {
		label,
		desc,
		segments: segs.length,
		hyp_chars: text.length,
		ref_chars: cerDetail.ref_chars,
		cer: cerDetail.cer,
		wer: cerDetail.wer,
		char_subs: cerDetail.char_subs ?? 0,
		char_ins: cerDetail.char_ins ?? 0,
		char_dels: cerDetail.char_dels ?? 0,
		recall_segs: `${coveredSegs}/${totalSegs}`,
		recall_chars: `${coveredChars}/${totalChars}`,
	};

	writeFileSync(join(resultDir, 'metadata', 'summary.json'), JSON.stringify(summary, null, 2));

	// Write ASR source for provenance
	const asrSrcPath = `packages/benchmark/asr/whisper/results/${ASR_SOURCE}/${ASR_PARAM}/metadata/whisper_raw.json`;
	writeFileSync(join(resultDir, 'metadata', 'asr.json'), JSON.stringify({
		_engine: 'whisper.cpp',
		_model: 'ggml-large-v3-turbo',
		_device: 'Vulkan (RADV)',
		_source: asrSrcPath,
		result: {
			text: asrSrc.map((s) => s.text).join(' '),
			segments: asrSrc.map((s) => ({
				text: s.text,
				start: s.start,
				end: s.end,
				start_fmt: formatTime(s.start),
				end_fmt: formatTime(s.end),
			})),
		},
	}, null, 2));

	const strategy = label.replace('ocr-asr-guided-', '');
	writeFileSync(
		join(resultDir, 'metadata', 'asr_ocr.json'),
		JSON.stringify({
			_engine: 'asr_ocr',
			_strategy: strategy,
			_asr: {
				engine: 'whisper.cpp',
				model: 'ggml-large-v3-turbo',
				device: 'Vulkan (RADV)',
				source: asrSrcPath,
			},
			_ocr: {
				engine: 'cpp-ort',
				model: 'rapidocr (det+cls+rec)',
				device: 'CPU',
			},
			_fusion_params: {
				strategy: strategy,
				ocrCalls: ocrCallCount ?? 0,
				elapsedSec: elapsedSec ? Math.round(elapsedSec * 10) / 10 : 0,
			},
			result: {
				text,
				segments: segs.map((s) => ({
					text: s.text,
					start: s.start,
					end: s.end,
					start_fmt: formatTime(s.start),
					end_fmt: formatTime(s.end),
				})),
			},
		}, null, 2),
	);

	console.log(`[${label}] ${desc}`);
	console.log(`  segs=${segs.length} hyp=${text.length} ref=${cerDetail.ref_chars}`);
	console.log(`  CER=${(cerDetail.cer * 100).toFixed(2)}% S=${cerDetail.char_subs} I=${cerDetail.char_ins} D=${cerDetail.char_dels}`);
	console.log(`  recall: ${coveredSegs}/${totalSegs} segs, ${coveredChars}/${totalChars} chars`);
	console.log();
}

if (require.main === module) {
	const gtSegs = loadGT();
	const asrSegs = loadASR();

	console.log('=== ASR-Guided OCR Benchmark ===');
	console.log(`ASR segments: ${asrSegs.length}`);
	console.log(`GT segments:  ${gtSegs.length}`);
	console.log();

	// --- Strategy 1: extract frame at ASR segment midpoint ---
	mkdirSync(TMP_DIR, { recursive: true });
	const midResults: { text: string; start: number; end: number }[] = [];
	let ocrCount = 0;
	let ocrStart = Date.now();

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const midMs = Math.round((asr.start + asr.end) / 2);
		const framePath = join(TMP_DIR, `frame_${i.toString().padStart(4, '0')}.jpg`);

		if (!extractFrame(VIDEO_PATH, midMs, framePath)) {
			console.warn(`  warn: frame extraction failed at ${midMs}ms`);
			continue;
		}

		const lines = ocrFrame(framePath);
		ocrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });

		if (best.text && best.confidence > 0.3) {
			midResults.push({ text: best.text, start: asr.start, end: asr.end });
		}
	}

	const midElapsed = (Date.now() - ocrStart) / 1000;
	const midSegs = mergeSegments(midResults, 1000);
	run('ocr-asr-guided-mid', midSegs, `ASR midpoint (${ocrCount} OCR calls, ${midElapsed.toFixed(1)}s)`, gtSegs, asrSegs, ocrCount, midElapsed);

	// --- Strategy 2: extract frame at ASR segment end ---
	const endResults: { text: string; start: number; end: number }[] = [];
	let endOcrCount = 0;
	let endStart = Date.now();

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const framePath = join(TMP_DIR, `end_${i.toString().padStart(4, '0')}.jpg`);

		if (!extractFrame(VIDEO_PATH, asr.end, framePath)) continue;

		const lines = ocrFrame(framePath);
		endOcrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });

		if (best.text && best.confidence > 0.3) {
			endResults.push({ text: best.text, start: asr.start, end: asr.end });
		}
	}

	const endElapsed = (Date.now() - endStart) / 1000;
	const endSegs = mergeSegments(endResults, 1000);
	run('ocr-asr-guided-end', endSegs, `ASR end time (${endOcrCount} OCR calls, ${endElapsed.toFixed(1)}s)`, gtSegs, asrSegs, endOcrCount, endElapsed);

	// --- Strategy 3: ASR midpoint but with subtitle-only crop ---
	const soResults: { text: string; start: number; end: number }[] = [];
	let soOcrCount = 0;
	let soStart = Date.now();

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const midMs = Math.round((asr.start + asr.end) / 2);
		const framePath = join(TMP_DIR, `so_${i.toString().padStart(4, '0')}.jpg`);

		if (!extractFrame(VIDEO_PATH, midMs, framePath)) continue;

		const r = spawnSync(CPP_BIN, [framePath, '0.5', '--subtitle-only'], {
			timeout: 60_000,
			encoding: 'utf-8',
			env: { ...process.env, LD_LIBRARY_PATH: CPP_LD_PATH },
		});
		soOcrCount++;
		if (r.status !== 0) continue;

		try {
			const parsed = JSON.parse(r.stdout);
			const lines: OCRLine[] = [];
			for (const seg of parsed.segments || []) {
				lines.push({ text: seg.text, confidence: seg.confidence });
			}
			if (lines.length === 0 && parsed.text) {
				lines.push({ text: parsed.text, confidence: 1 });
			}
			const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
			if (best.text && best.confidence > 0.3) {
				soResults.push({ text: best.text, start: asr.start, end: asr.end });
			}
		} catch {}
	}

	const soElapsed = (Date.now() - soStart) / 1000;
	const soSegs = mergeSegments(soResults, 1000);
	run('ocr-asr-guided-mid-so', soSegs, `ASR midpoint + subtitle-only (${soOcrCount} OCR calls, ${soElapsed.toFixed(1)}s)`, gtSegs, asrSegs, soOcrCount, soElapsed);

	// --- Strategy 4: For each ASR segment, extract frames at 1fps intervals ---
	const fpsResults: { text: string; start: number; end: number; time: number }[] = [];
	let fpsOcrCount = 0;
	let fpsOcrStart = Date.now();

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const startSec = Math.ceil(asr.start / 1000);
		const endSec = Math.floor(asr.end / 1000);

		for (let t = startSec; t <= endSec; t++) {
			const frameMs = t * 1000;
			const framePath = join(TMP_DIR, `fps1_${i.toString().padStart(4, '0')}_${t}.jpg`);

			if (!extractFrame(VIDEO_PATH, frameMs, framePath)) continue;

			const lines = ocrFrame(framePath);
			fpsOcrCount++;
			const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });

			if (best.text && best.confidence > 0.3) {
				fpsResults.push({ text: best.text, start: frameMs, end: frameMs, time: frameMs });
			}
		}
	}

	// Dedup consecutive same-text from same ASR segment
	const deduped: { text: string; start: number; end: number }[] = [];
	for (const r of fpsResults) {
		const last = deduped[deduped.length - 1];
		if (last && last.text === r.text) {
			last.end = Math.max(last.end, r.time);
		} else {
			deduped.push({ text: r.text, start: r.time, end: r.time });
		}
	}

	const fpsElapsed = (Date.now() - fpsOcrStart) / 1000;
	const fpsSegs = mergeSegments(deduped, 1000);
	run('ocr-asr-guided-fps1', fpsSegs, `ASR 1fps intervals (${fpsOcrCount} OCR calls, ${fpsElapsed.toFixed(1)}s)`, gtSegs, asrSegs, fpsOcrCount, fpsElapsed);

	// --- Strategy 5: ASR midpoint ±1fps, clamped to ASR segment bounds ---
	const midFpsResults: { text: string; start: number; end: number; time: number }[] = [];
	let midFpsOcrCount = 0;
	let midFpsOcrStart = Date.now();

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];

		// Generate frames at 1fps from midpoint, expanding outward, clamped to [start, end]
		const midMs = Math.round((asr.start + asr.end) / 2);
		const frames: number[] = [];
		for (let t = midMs; t <= asr.end; t += 1000) frames.push(Math.round(t));
		for (let t = midMs - 1000; t >= asr.start; t -= 1000) frames.push(Math.round(t));
		frames.sort((a, b) => a - b);

		for (const frameMs of frames) {
			const framePath = join(TMP_DIR, `midfps_${i.toString().padStart(4, '0')}_${frameMs}.jpg`);

			if (!extractFrame(VIDEO_PATH, frameMs, framePath)) continue;

			const lines = ocrFrame(framePath);
			midFpsOcrCount++;
			const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });

			if (best.text && best.confidence > 0.3) {
				midFpsResults.push({ text: best.text, start: frameMs, end: frameMs, time: frameMs });
			}
		}
	}

	// Dedup consecutive same-text (within same ASR segment)
	const midFpsDeduped: { text: string; start: number; end: number }[] = [];
	for (const r of midFpsResults) {
		const last = midFpsDeduped[midFpsDeduped.length - 1];
		if (last && last.text === r.text) {
			last.end = Math.max(last.end, r.time);
		} else {
			midFpsDeduped.push({ text: r.text, start: r.time, end: r.time });
		}
	}

	const midFpsElapsed = (Date.now() - midFpsOcrStart) / 1000;
	const midFpsSegs = mergeSegments(midFpsDeduped, 1000);
	run(`ocr-asr-guided-midfps-${ASR_SOURCE}-${ASR_PARAM}`, midFpsSegs, `ASR midpoint ±1fps clamped (${midFpsOcrCount} OCR calls, ${midFpsElapsed.toFixed(1)}s)`, gtSegs, asrSegs, midFpsOcrCount, midFpsElapsed);

	// --- Compare with fixed-fps OCR ---
	console.log('======= SUMMARY =======');
	console.log('label                         | segs | hyp_ch | ref_ch | CER%   | S  | I  | D  | recall_seg | recall_ch');
	console.log('-----------------------------|------|--------|--------|--------|----|----|----|------------|-----------');
	for (const label of ['ocr-asr-guided-mid', 'ocr-asr-guided-end', 'ocr-asr-guided-mid-so', 'ocr-asr-guided-fps1', `ocr-asr-guided-midfps-${ASR_SOURCE}-${ASR_PARAM}`]) {
		const p = join(RESULTS_BASE, label, 'metadata', 'summary.json');
		if (!existsSync(p)) continue;
		const s = JSON.parse(readFileSync(p, 'utf-8'));
		const l = (s.label || label).padEnd(27);
		const seg = String(s.segments).padStart(4);
		const hc = String(s.hyp_chars).padStart(6);
		const rc = String(s.ref_chars).padStart(6);
		const cer = (s.cer * 100).toFixed(2).padStart(6);
		const sub = String(s.char_subs ?? 0).padStart(2);
		const ins = String(s.char_ins ?? 0).padStart(2);
		const del = String(s.char_dels ?? 0).padStart(2);
		const rs = String(s.recall_segs ?? '?').padStart(10);
		const rc2 = String(s.recall_chars ?? '?').padStart(10);
		console.log(`${l} | ${seg} | ${hc} | ${rc} | ${cer}% | ${sub} | ${ins} | ${del} | ${rs} | ${rc2}`);
	}
	console.log();
	console.log('Reference (OCR-only, fixed global 1fps):');
	console.log('  CER=0.87% S=1 I=0 D=4 recall=91/93 segs 554/572 chars (170 OCR calls)');
}
