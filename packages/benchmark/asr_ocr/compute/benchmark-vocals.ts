import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const REPO_ROOT = resolve(__dirname, '..', '..', '..', '..');
const WER_PY = resolve(REPO_ROOT, 'packages', 'benchmark', 'separate', 'compute', 'wer.py');
const CPP_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'metadata', 'srt_manual.json');
const VIDEO_PATH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'media', 'video_source.mp4');
const VOCALS_ASR = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr_ocr', 'results', 'sep-asr-ocr', 'metadata', 'asr.json');
const RESULTS_BASE = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr_ocr', 'results', 'sep-asr-ocr');
const TMP_DIR = resolve(REPO_ROOT, 'packages', 'tmp', 'vocals-guided-ocr');

interface Segment {
	text: string;
	start: number;
	end: number;
}

interface ASRSeg extends Segment {}
interface OCRLine {
	text: string;
	confidence: number;
}

function loadGT(): Segment[] {
	return JSON.parse(readFileSync(GROUND_TRUTH, 'utf-8')).result.segments;
}

function loadVocalsASR(): ASRSeg[] {
	const raw = JSON.parse(readFileSync(VOCALS_ASR, 'utf-8'));
	return raw.result.segments.map((s: any) => ({
		text: s.text,
		start: s.start,
		end: s.end,
	}));
}

function ocrFrame(framePath: string): OCRLine[] {
	const r = spawnSync(CPP_BIN, [framePath, '0.5', '--subtitle-only'], {
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

function run(label: string, segs: Segment[], desc: string, gtSegs: Segment[], asrSrc: ASRSeg[], ocrCallCount?: number, elapsedSec?: number) {
	const text = segmentsToText(segs);
	const cerDetail = calcCERDetail(text);

	const resultDir = join(RESULTS_BASE, label);
	mkdirSync(resultDir, { recursive: true });
	mkdirSync(join(resultDir, 'metadata'), { recursive: true });

	let coveredSegs = 0;
	let coveredChars = 0;
	for (const g of gtSegs) {
		if (!g.text) continue;
		const covered = segs.some((h) =>
			h.start <= g.end + 1000 && h.end >= g.start - 1000 &&
			([...g.text].some((c) => h.text.includes(c)) || g.text.length < 3)
		);
		if (covered) { coveredSegs++; coveredChars += g.text.length; }
	}
	const totalSegs = gtSegs.filter((s) => s.text).length;
	const totalChars = gtSegs.reduce((a, s) => a + s.text.length, 0);

	const summary = {
		label, desc, segments: segs.length, hyp_chars: text.length, ref_chars: cerDetail.ref_chars,
		cer: cerDetail.cer, wer: cerDetail.wer, char_subs: cerDetail.char_subs ?? 0,
		char_ins: cerDetail.char_ins ?? 0, char_dels: cerDetail.char_dels ?? 0,
		recall_segs: `${coveredSegs}/${totalSegs}`, recall_chars: `${coveredChars}/${totalChars}`,
	};

	writeFileSync(join(resultDir, 'metadata', 'summary.json'), JSON.stringify(summary, null, 2));

	// Write ASR source
	writeFileSync(join(resultDir, 'metadata', 'asr.json'), JSON.stringify({
		_engine: 'whisper.cpp', _model: 'ggml-large-v3-turbo',
		_device: 'Vulkan (RADV)',
		_input: 'packages/benchmark/asr_ocr/results/sep-asr-ocr/media/target_3_vocals.wav',
		_params: { language: 'zh', source_type: 'vocals (demucs separated)' },
		result: {
			text: asrSrc.map((s) => s.text).join(' '),
			segments: asrSrc.map((s) => ({
				text: s.text, start: s.start, end: s.end,
				start_fmt: formatTime(s.start), end_fmt: formatTime(s.end),
			})),
		},
	}, null, 2));

	const strategy = label.replace('sep-ocr-', '');
	writeFileSync(join(resultDir, 'metadata', 'asr_ocr.json'), JSON.stringify({
		_engine: 'asr_ocr', _strategy: strategy,
		_asr: { engine: 'whisper.cpp', model: 'ggml-large-v3-turbo', device: 'Vulkan (RADV)',
			source: 'packages/benchmark/asr_ocr/results/sep-asr-ocr/metadata/asr.json' },
		_ocr: { engine: 'cpp-ort', model: 'rapidocr (det+cls+rec)', device: 'CPU' },
		_fusion_params: { strategy, ocrCalls: ocrCallCount ?? 0, elapsedSec: elapsedSec ? Math.round(elapsedSec * 10) / 10 : 0 },
		result: {
			text,
			segments: segs.map((s) => ({
				text: s.text, start: s.start, end: s.end,
				start_fmt: formatTime(s.start), end_fmt: formatTime(s.end),
			})),
		},
	}, null, 2));

	console.log(`[${label}] ${desc}`);
	console.log(`  segs=${segs.length} hyp=${text.length} ref=${cerDetail.ref_chars}`);
	console.log(`  CER=${(cerDetail.cer * 100).toFixed(2)}% S=${cerDetail.char_subs} I=${cerDetail.char_ins} D=${cerDetail.char_dels}`);
	console.log(`  recall: ${coveredSegs}/${totalSegs} segs, ${coveredChars}/${totalChars} chars`);
	console.log();
}

if (require.main === module) {
	const gtSegs = loadGT();
	const asrSegs = loadVocalsASR();

	console.log('=== Vocals ASR-Guided OCR Benchmark ===');
	console.log(`ASR segments (vocals): ${asrSegs.length}`);
	console.log(`GT segments:           ${gtSegs.length}`);
	console.log();

	mkdirSync(TMP_DIR, { recursive: true });

	// --- Pre-extract all frames at 5fps (200ms precision for ASR-guided strategies) ---
	console.log("Extracting frames at 5fps...");
	const allFramesDir = join(TMP_DIR, 'frames');
	mkdirSync(allFramesDir, { recursive: true });
	spawnSync('ffmpeg', [
		'-y', '-i', VIDEO_PATH,
		'-vf', 'fps=5',
		'-qscale:v', '2',
		join(allFramesDir, 'frame_%05d.jpg'),
	], { timeout: 60_000, encoding: 'utf-8' });
	const frameFiles = readdirSync(allFramesDir).filter((f) => f.endsWith('.jpg')).sort();
	console.log(`  ${frameFiles.length} frames extracted`);

	function frameAt(sec: number): string | null {
		const idx = Math.round(sec * 5) + 1;
		const f = `frame_${idx.toString().padStart(5, '0')}.jpg`;
		const p = join(allFramesDir, f);
		return existsSync(p) ? p : null;
	}

	// --- Fixed 1fps OCR baseline (sub-sample 5fps frames at integer seconds) ---
	let ocrStart = Date.now();
	const fixedResults: { text: string; start: number; end: number }[] = [];
	const totalFrames = Math.ceil(gtSegs[gtSegs.length - 1].end / 1000);
	let fixedOcrCount = 0;

	for (let t = 0; t <= totalFrames; t++) {
		const frameMs = t * 1000;
		const fp = frameAt(t);
		if (!fp) continue;
		const lines = ocrFrame(fp);
		fixedOcrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
		if (best.text && best.confidence > 0.3) {
			const last = fixedResults[fixedResults.length - 1];
			if (last && last.text === best.text) {
				last.end = frameMs + 1000;
			} else {
				fixedResults.push({ text: best.text, start: frameMs, end: frameMs + 1000 });
			}
		}
	}
	const fixedElapsed = (Date.now() - ocrStart) / 1000;
	run('sep-ocr-fixed-1fps', fixedResults, `Fixed 1fps (${fixedOcrCount} OCR calls, ${fixedElapsed.toFixed(1)}s)`, gtSegs, asrSegs, fixedOcrCount, fixedElapsed);

	// --- Strategy 1: ASR midpoint (nearest 5fps frame) ---
	ocrStart = Date.now();
	const midResults: { text: string; start: number; end: number }[] = [];
	let ocrCount = 0;

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const midMs = Math.round((asr.start + asr.end) / 2);
		const fp = frameAt(midMs / 1000);
		if (!fp) continue;
		const lines = ocrFrame(fp);
		ocrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
		if (best.text && best.confidence > 0.3) {
			midResults.push({ text: best.text, start: asr.start, end: asr.end });
		}
	}
	const midElapsed = (Date.now() - ocrStart) / 1000;
	run('sep-ocr-guided-mid', mergeSegments(midResults), `ASR midpoint (${ocrCount} OCR calls, ${midElapsed.toFixed(1)}s)`, gtSegs, asrSegs, ocrCount, midElapsed);

	// --- Strategy 2: ASR end (nearest 5fps frame) ---
	ocrStart = Date.now();
	const endResults: { text: string; start: number; end: number }[] = [];
	let endOcrCount = 0;
	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const fp = frameAt(asr.end / 1000);
		if (!fp) continue;
		const lines = ocrFrame(fp);
		endOcrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
		if (best.text && best.confidence > 0.3) {
			endResults.push({ text: best.text, start: asr.start, end: asr.end });
		}
	}
	const endElapsed = (Date.now() - ocrStart) / 1000;
	run('sep-ocr-guided-end', mergeSegments(endResults), `ASR end (${endOcrCount} OCR calls, ${endElapsed.toFixed(1)}s)`, gtSegs, asrSegs, endOcrCount, endElapsed);

	// --- Strategy 3: ASR midpoint (5fps, declared subtitle-only, same as ocrFrame default) ---
	ocrStart = Date.now();
	const soResults: { text: string; start: number; end: number }[] = [];
	let soOcrCount = 0;
	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const midMs = Math.round((asr.start + asr.end) / 2);
		const fp = frameAt(midMs / 1000);
		if (!fp) continue;
		const lines = ocrFrame(fp);
		soOcrCount++;
		const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
		if (best.text && best.confidence > 0.3) {
			soResults.push({ text: best.text, start: asr.start, end: asr.end });
		}
	}
	const soElapsed = (Date.now() - ocrStart) / 1000;
	run('sep-ocr-guided-mid-so', mergeSegments(soResults), `ASR midpoint (${soOcrCount} OCR calls, ${soElapsed.toFixed(1)}s)`, gtSegs, asrSegs, soOcrCount, soElapsed);

	// --- Strategy 4: 5fps within ASR segments ---
	ocrStart = Date.now();
	const fpsResults: { text: string; start: number; end: number; time: number }[] = [];
	let fpsOcrCount = 0;

	for (let i = 0; i < asrSegs.length; i++) {
		const asr = asrSegs[i];
		const startFrame = Math.ceil(asr.start / 200);
		const endFrame = Math.floor(asr.end / 200);
		for (let fi = startFrame; fi <= endFrame; fi++) {
			const frameMs = fi * 200;
			const fp = frameAt(frameMs / 1000);
			if (!fp) continue;
			const lines = ocrFrame(fp);
			fpsOcrCount++;
			const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
			if (best.text && best.confidence > 0.3) {
				const last = fpsResults[fpsResults.length - 1];
				if (last && last.text === best.text && last.time + 200 >= frameMs) {
					last.end = Math.max(last.end, frameMs + 200);
				} else {
					fpsResults.push({ text: best.text, start: frameMs, end: frameMs + 200, time: frameMs });
				}
			}
		}
	}
	const fpsElapsed = (Date.now() - ocrStart) / 1000;
	const fpsSegs: Segment[] = fpsResults.map((r) => ({ text: r.text, start: r.start, end: r.end }));
	run('sep-ocr-guided-fps1', mergeSegments(fpsSegs, 200), `ASR 5fps intervals (${fpsOcrCount} OCR calls, ${fpsElapsed.toFixed(1)}s)`, gtSegs, asrSegs, fpsOcrCount, fpsElapsed);

	// --- Summary ---
	console.log('======= VOCALS ASR-GUIDED OCR SUMMARY =======');
	console.log('label                      | segs | hyp_ch | ref_ch | CER%   | S  | I  | D  | recall_seg | recall_ch');
	console.log('---------------------------|------|--------|--------|--------|----|----|----|------------|-----------');
	const labels = ['sep-ocr-fixed-1fps', 'sep-ocr-guided-mid', 'sep-ocr-guided-end', 'sep-ocr-guided-mid-so', 'sep-ocr-guided-fps1'];
	for (const label of labels) {
		const p = join(RESULTS_BASE, label, 'metadata', 'summary.json');
		if (!existsSync(p)) continue;
		const s = JSON.parse(readFileSync(p, 'utf-8'));
		const l = (s.label || label).padEnd(25);
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
}
