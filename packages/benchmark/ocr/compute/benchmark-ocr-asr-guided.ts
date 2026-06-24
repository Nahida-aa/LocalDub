// ASR-guided OCR benchmark (mid1fps strategy only)
// Usage: ASR_SOURCE=ggml-s1-vocals ASR_PARAM=baseline \
//   bun run benchmark-ocr-asr-guided.ts --text-score 0.45 --subtitle-only --label sep-vocals-asr-guided-ocr-so-ts0.45-mid1fps
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { levenshtein, FrameResult, mergeFrames, dedupOverlap } from '../../../cli/src/feat/stages/utils/ocrMerge';

const REPO_ROOT = resolve(__dirname, '..', '..', '..', '..');
const CPP_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const VIDEO_PATH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'media', 'video_source.mp4');
const ASR_SOURCE = process.env.ASR_SOURCE || 'ggml-s1-vocals';
const ASR_PARAM = process.env.ASR_PARAM || 'baseline';
const ASR_RAW = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr', 'whisper', 'results', ASR_SOURCE, ASR_PARAM, 'metadata', 'whisper_raw.json');
const RESULTS_BASE = resolve(__dirname, '..', 'results');
const TMP_DIR = resolve(REPO_ROOT, 'packages', 'tmp', 'ocr-asr-guided');

interface Segment { text: string; start: number; end: number }
interface ASRSeg extends Segment { text: string }
interface OCRLine { text: string; confidence: number }

let TEXT_SCORE = 0.45;
let SUBTITLE_ONLY = true;
let LABEL = '';
let STRATEGY = 'mid1fps';

function parseArgs() {
  const args = process.argv.slice(2);
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--text-score') TEXT_SCORE = parseFloat(args[++i]);
    if (args[i] === '--subtitle-only') SUBTITLE_ONLY = true;
    if (args[i] === '--label') LABEL = args[++i];
    if (args[i] === '--strategy') STRATEGY = args[++i];
    if (args[i] === '--help') {
      console.log('Usage: ASR_SOURCE=... ASR_PARAM=... bun run benchmark-ocr-asr-guided.ts [--text-score 0.45] [--subtitle-only] [--strategy mid1fps|end1fps|end2fps] [--label name]');
      process.exit(0);
    }
  }
  if (!LABEL) {
    const so = SUBTITLE_ONLY ? '-so' : '';
    const ts = TEXT_SCORE != null ? `-ts${String(TEXT_SCORE).replace('.', '')}` : '';
    LABEL = `asr-guided-${ASR_SOURCE}-${ASR_PARAM}${so}${ts}-${STRATEGY}`;
  }
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
  const args = [framePath];
  if (TEXT_SCORE != null) args.push(String(TEXT_SCORE));
  if (SUBTITLE_ONLY) args.push('--subtitle-only');
  const r = spawnSync(CPP_BIN, args, {
    timeout: 60_000,
    encoding: 'utf-8',
    env: { ...process.env, LD_LIBRARY_PATH: CPP_LD_PATH },
  });
  if (r.status !== 0) return [];
  try {
    const parsed = JSON.parse(r.stdout);
    const lines: OCRLine[] = [];
    for (const seg of parsed.segments || []) lines.push({ text: seg.text, confidence: seg.confidence });
    if (lines.length === 0 && parsed.text) lines.push({ text: parsed.text, confidence: 1 });
    return lines;
  } catch { return []; }
}

function mergeSegments(segs: Segment[], sameTextGapMs = 1000): Segment[] {
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
  return dedupOverlap(merged);
}

function segmentsToText(segs: Segment[]): string { return segs.map((s) => s.text).join(''); }

function formatTime(ms: number): string {
  const h = Math.floor(ms / 3600000);
  const m = Math.floor((ms % 3600000) / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const ml = ms % 1000;
  return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')},${ml.toString().padStart(3, '0')}`;
}

function extractFrame(videoPath: string, timeMs: number, outPath: string): boolean {
  const r = spawnSync('ffmpeg', ['-y', '-ss', String(timeMs / 1000), '-i', videoPath, '-frames:v', '1', '-qscale:v', '2', outPath],
    { timeout: 10_000, encoding: 'utf-8' });
  return r.status === 0;
}

function fixOverlap(asrSegs: Segment[], rawFrames: FrameResult[], ocrSegs: Segment[]): Segment[] {
  const fix = asrSegs.map((s) => ({ ...s }));
  const sorted = [...rawFrames].sort((a, b) => a.timestamp - b.timestamp);

  // Pass 1: fix overlapping ASR segments using OCR frame text change points
  for (let i = 1; i < fix.length; i++) {
    const prev = fix[i - 1];
    const cur = fix[i];
    if (cur.start >= prev.end) continue;
    const overlapEnd = Math.min(prev.end, cur.end);
    for (const f of sorted) {
      if (f.timestamp < cur.start) continue;
      if (f.timestamp > overlapEnd) break;
      const dCur = levenshtein(f.text, cur.text);
      const dPrev = levenshtein(f.text, prev.text);
      if (dCur <= 2 && dCur < dPrev) {
        prev.end = f.timestamp;
        cur.start = f.timestamp;
        break;
      }
    }
  }

  // Pass 2: if ASR start is > 500ms before the matching OCR segment start, clip to OCR start
  for (const seg of fix) {
    let bestOcr: Segment | null = null;
    let bestOverlap = 0;
    for (const o of ocrSegs) {
      const overlap = Math.min(seg.end, o.end) - Math.max(seg.start, o.start);
      if (overlap > bestOverlap && levenshtein(seg.text, o.text) <= 2) {
        bestOverlap = overlap;
        bestOcr = o;
      }
    }
    if (bestOcr && seg.start + 500 < bestOcr.start) {
      seg.start = bestOcr.start;
    }
  }

  return fix;
}

function run(label: string, ocrSegs: Segment[], asrSegs: Segment[], rawFrames: FrameResult[], desc: string, asrSrc: ASRSeg[], ocrCallCount?: number, elapsedSec?: number) {
  const ocrText = segmentsToText(ocrSegs);
  const asrText = segmentsToText(asrSegs);
  const resultDir = join(RESULTS_BASE, label);
  mkdirSync(resultDir, { recursive: true });
  mkdirSync(join(resultDir, 'metadata'), { recursive: true });

  const asrSrcPath = `packages/benchmark/asr/whisper/results/${ASR_SOURCE}/${ASR_PARAM}/metadata/whisper_raw.json`;

  // asr_ocr_fix — use OCR frame data to find text change points in overlapping regions
  const fixSegs = fixOverlap(asrSegs, rawFrames, ocrSegs).filter((s) => s.end > s.start);
  const fixText = segmentsToText(fixSegs);

  // summary.json — lightweight bench stats
  writeFileSync(join(resultDir, 'metadata', 'summary.json'), JSON.stringify({ label, desc, ocrSegments: ocrSegs.length, asrSegments: asrSegs.length, fixSegments: fixSegs.length, ocrHypChars: ocrText.length, asrHypChars: asrText.length, fixHypChars: fixText.length, ocrCalls: ocrCallCount ?? 0, elapsedSec: elapsedSec ?? 0 }, null, 2));

  // ocr.json — pure OCR boundaries (for eval-ocr.ts --ms)
  writeFileSync(join(resultDir, 'metadata', 'ocr.json'), JSON.stringify({
    audio_info: { duration: ocrSegs.length > 0 ? ocrSegs[ocrSegs.length - 1].end : 0 },
    _boundary: 'ocr',
    result: { text: ocrText, segments: ocrSegs.map((s) => ({ text: s.text, start: s.start, end: s.end })) },
  }, null, 2));

  // asr_ocr.json — ASR-guided boundaries (dev reference)
  writeFileSync(join(resultDir, 'metadata', 'asr_ocr.json'), JSON.stringify({
    _engine: 'asr_ocr', _strategy: STRATEGY, _boundary: 'asr',
    _asr: { engine: 'whisper.cpp', model: 'ggml-large-v3-turbo', device: 'Vulkan (RADV)', source: asrSrcPath },
    _ocr: { engine: 'cpp-ort', model: 'rapidocar (det+cls+rec)', device: 'CPU', textScore: TEXT_SCORE, subtitleOnly: SUBTITLE_ONLY },
    _fusion_params: { strategy: STRATEGY, ocrCalls: ocrCallCount ?? 0, elapsedSec: elapsedSec ? Math.round(elapsedSec * 10) / 10 : 0 },
    result: { text: asrText, segments: asrSegs.map((s) => ({ text: s.text, start: s.start, end: s.end, start_fmt: formatTime(s.start), end_fmt: formatTime(s.end) })) },
  }, null, 2));

  // asr_ocr_fix.json — ASR-guided with overlap clipped (dev reference)
  writeFileSync(join(resultDir, 'metadata', 'asr_ocr_fix.json'), JSON.stringify({
    _engine: 'asr_ocr', _strategy: STRATEGY, _boundary: 'asr_fix',
    _asr: { engine: 'whisper.cpp', model: 'ggml-large-v3-turbo', device: 'Vulkan (RADV)', source: asrSrcPath },
    _ocr: { engine: 'cpp-ort', model: 'rapidocar (det+cls+rec)', device: 'CPU', textScore: TEXT_SCORE, subtitleOnly: SUBTITLE_ONLY },
    _fusion_params: { strategy: STRATEGY, ocrCalls: ocrCallCount ?? 0, elapsedSec: elapsedSec ? Math.round(elapsedSec * 10) / 10 : 0 },
    result: { text: fixText, segments: fixSegs.map((s) => ({ text: s.text, start: s.start, end: s.end, start_fmt: formatTime(s.start), end_fmt: formatTime(s.end) })) },
  }, null, 2));

  console.log(`[${label}] ${desc}`);
  console.log(`  OCR-boundary: segs=${ocrSegs.length} hyp=${ocrText.length}`);
  console.log(`  ASR-boundary: segs=${asrSegs.length} hyp=${asrText.length}`);
  console.log(`  ASR-fix:      segs=${fixSegs.length} hyp=${fixText.length}`);
  console.log(`  calls=${ocrCallCount} in ${elapsedSec?.toFixed(1)}s`);
}

if (require.main === module) {
  parseArgs();
  const asrSegs = loadASR();

  console.log(`=== ASR-Guided OCR: ${STRATEGY} ===`);
  console.log(`ASR source: ${ASR_SOURCE}/${ASR_PARAM} (${asrSegs.length} segments)`);
  console.log(`OCR: textScore=${TEXT_SCORE}, subtitleOnly=${SUBTITLE_ONLY}`);
  console.log(`Label: ${LABEL}`);
  console.log();

  mkdirSync(TMP_DIR, { recursive: true });

  // ASR-guided results: text + ASR segment boundaries
  const asrResults: { text: string; start: number; end: number }[] = [];
  // Pure frame-level data for OCR boundary detection
  const rawFrames: FrameResult[] = [];
  let ocrCount = 0;
  let ocrStart = Date.now();

  for (let i = 0; i < asrSegs.length; i++) {
    const asr = asrSegs[i];
    const frames: number[] = [];
    if (i === 0) {
      // 第一段 10fps，精确找字幕首次出现帧
      for (let t = Math.round(asr.start); t <= Math.round(asr.end); t += 100) frames.push(Math.round(t));
    } else if (STRATEGY === 'end1fps') {
      for (let t = Math.round(asr.end) - 200; t >= asr.start; t -= 1000) frames.push(Math.round(t));
      frames.sort((a, b) => a - b);
    } else if (STRATEGY === 'end2fps') {
      for (let t = Math.round(asr.end); t >= asr.start; t -= 500) frames.push(Math.round(t));
      frames.sort((a, b) => a - b);
    } else {
      const midMs = Math.round((asr.start + asr.end) / 2);
      for (let t = midMs; t <= asr.end; t += 1000) frames.push(Math.round(t));
      for (let t = midMs - 1000; t >= asr.start; t -= 1000) frames.push(Math.round(t));
      frames.sort((a, b) => a - b);
    }

    for (const frameMs of frames) {
      const framePath = join(TMP_DIR, `${STRATEGY}_${i.toString().padStart(4, '0')}_${frameMs}.jpg`);
      if (!extractFrame(VIDEO_PATH, frameMs, framePath)) continue;

      const lines = ocrFrame(framePath);
      ocrCount++;
      const best = lines.reduce((a, b) => a.confidence > b.confidence ? a : b, { text: '', confidence: 0 });
      const threshold = TEXT_SCORE ?? 0.3;
      if (best.text && best.confidence > threshold) {
        asrResults.push({ text: best.text, start: i === 0 ? Math.max(0, frameMs - 90) : asr.start, end: asr.end });
        rawFrames.push({ text: best.text, timestamp: frameMs, confidence: best.confidence });
      }
    }
  }

  // --- ASR-guided boundaries ---
  const deduped: { text: string; start: number; end: number }[] = [];
  for (const r of asrResults) {
    const last = deduped[deduped.length - 1];
    if (last && last.text === r.text && r.start <= last.end + 1000) {
      last.start = Math.min(last.start, r.start);
      last.end = Math.max(last.end, r.end);
    } else {
      deduped.push({ text: r.text, start: r.start, end: r.end });
    }
  }
  const asrSegs_merged = mergeSegments(deduped, 1000);

  // --- Pure OCR boundaries (from frame timestamps) ---
  rawFrames.sort((a, b) => a.timestamp - b.timestamp);
  const ocrSegs_merged = mergeFrames(rawFrames).map((s) => ({ text: s.text, start: s.start, end: s.end }));

  const elapsed = (Date.now() - ocrStart) / 1000;
  const hop = STRATEGY === 'end1fps' ? 'end-200ms 1fps clamped' : 'mid ±1fps clamped';
  run(LABEL, ocrSegs_merged, asrSegs_merged, rawFrames, `ASR ${hop} (${ocrCount} OCR calls, ${elapsed.toFixed(1)}s)`, asrSegs, ocrCount, elapsed);

  spawnSync('rm', ['-rf', TMP_DIR]);
  console.log(`\nResults: ${join(RESULTS_BASE, LABEL)}`);
  console.log(`Eval: bun run packages/benchmark/ref/compute/eval-ocr.ts packages/benchmark/ocr/results/${LABEL}/metadata/ocr.json --label ${LABEL} --ms
  (or: --write to save ocr_summary.json)`);
}
