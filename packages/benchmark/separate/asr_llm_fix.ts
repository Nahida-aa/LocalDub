import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const RESULTS_DIR = join(__dirname, 'results');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr_manual.json');
const PYTHON_BIN = join(REPO_ROOT, '.venv', 'bin', 'python');
const WER_PY = join(__dirname, 'wer.py');

const API_BASE = 'http://localhost:11434/v1';
const MODEL = 'gemma4:31b-cloud';

function srtTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  const ms = Math.round((seconds - Math.floor(seconds)) * 1000);
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')},${String(ms).padStart(3, '0')}`;
}

function segmentsToSRT(segments: any[]): string {
  return segments.map((s, i) =>
    `${i + 1}\n${srtTime(s.start)} --> ${srtTime(s.end)}\n${s.text}\n`
  ).join('');
}

function parseSRT(srt: string, expectedCount: number): string[] | null {
  const texts: string[] = [];
  // Strip markdown code fences
  let cleaned = srt.replace(/```srt\n?/gi, '').replace(/```\n?/g, '');
  const blocks = cleaned.trim().split(/\n\n+/);
  for (const block of blocks) {
    const lines = block.trim().split('\n');
    if (lines.length < 3) continue;
    if (!/^\d+$/.test(lines[0].trim())) continue;
    if (!/-->/.test(lines[1])) continue;
    texts.push(lines.slice(2).join('\n').trim());
  }
  if (texts.length !== expectedCount) return null;
  return texts;
}

const SYSTEM_PROMPT = `你是一个 ASR 纠错助手。修正中文 SRT 字幕中的错别字，严格遵循以下规则：

这是一部中国仙侠/修仙题材动画的对话转录：
- 角色名：叶白、慧天（王慧天）、夜白
- 修仙术语：灵石、灵根、剑仙、心性定力、剑道天赋、剑法、神识、灵气
- 常见错例："零食"→"灵石"，"修为尚寝"→"修为尚浅"，"拜剑师祖"→"拜见师祖"，"王会天"→"王慧天"，"资质尚承"→"资质上乘"

你必须严格遵守：
1. 保持序号和时间轴完全不变
2. 只修改文字行的错别字，不改标点、不改空格
3. 不要合并或拆分条目，保持行数一致
4. 不要添加任何解释、前言、后记
5. 不要使用 markdown 代码块
6. 输出纯文本 SRT，不要任何其他内容`;

async function fixWithLLM(srt: string): Promise<string> {
  const resp = await fetch(`${API_BASE}/chat/completions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model: MODEL,
      max_tokens: 4096,
      temperature: 0.1,
      messages: [
        { role: 'system', content: SYSTEM_PROMPT },
        { role: 'user', content: srt },
      ],
    }),
  });

  if (!resp.ok) {
    const err = await resp.text();
    throw new Error(`LLM API ${resp.status}: ${err.slice(0, 200)}`);
  }

  const json = await resp.json();
  return (json.choices?.[0]?.message?.content || '').trim();
}

function computeWER(refFile: string, hypFile: string): { wer: number; cer: number } {
  const r = spawnSync(PYTHON_BIN, [WER_PY, refFile, hypFile], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`wer.py failed: ${r.stderr?.toString().slice(-200)}`);
  return JSON.parse(r.stdout.toString());
}

interface FileEntry { label: string; file: string }

const FILES: FileEntry[] = [
  { label: 'raw', file: 'wer-raw-video.json' },
  { label: 'ggml', file: 'wer-ggml-shifts1-16bit.json' },
  { label: 'ort', file: 'wer-ort-video.json' },
  { label: 'pytorch-s1', file: 'wer-pytorch-shifts1.json' },
  { label: 'pytorch-s3', file: 'wer-pytorch-shifts3.json' },
];

interface Result {
  label: string;
  cerBefore: number; cerAfter: number; werBefore: number; werAfter: number;
  segmentsBefore: number; segmentsAfter: number; fallback: boolean;
}

async function main() {
  mkdirSync(RESULTS_DIR, { recursive: true });

  if (!existsSync(GROUND_TRUTH)) {
    console.error('Ground truth not found');
    process.exit(1);
  }

  const results: Result[] = [];

  for (const { label, file } of FILES) {
    const filePath = join(RESULTS_DIR, file);
    if (!existsSync(filePath)) {
      console.warn(`  SKIP: ${file} not found`);
      continue;
    }

    const data = JSON.parse(readFileSync(filePath, 'utf-8'));
    const segments: any[] = data.result?.segments || [];
    const text = data.result?.text || '';
    const srt = segmentsToSRT(segments);

    console.log(`[${label}] ${segments.length} segs, sending SRT (${srt.length} chars)...`);
    const t0 = performance.now();
    const fixed = await fixWithLLM(srt);
    const elapsed = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseSRT(fixed, segments.length);
    let resultSegments: any[];
    let resultText: string;
    let fallback = false;

    if (fixedTexts) {
      resultSegments = segments.map((s: any, i: number) => ({ ...s, text: fixedTexts[i] }));
      resultText = fixedTexts.join(' ');
      console.log(`  Done in ${elapsed}s, parsed ${fixedTexts.length} segs OK`);
    } else {
      resultSegments = segments;
      resultText = text;
      fallback = true;
      console.log(`  Done in ${elapsed}s, PARSE FAILED (${
        (fixed.match(/\n/g) || []).length + 1
      } lines), using original`);
    }

    const fixedFile = join(RESULTS_DIR, `wer-${label}-llm-fixed.json`);
    writeFileSync(fixedFile, JSON.stringify({
      audio_info: data.audio_info || {},
      result: { text: resultText, segments: resultSegments },
      _device: data._device || 'cpu',
      _llm_fixed: true,
    }, null, 2));

    const origResult = computeWER(GROUND_TRUTH, filePath);
    const fixedResult = computeWER(GROUND_TRUTH, fixedFile);
    results.push({
      label, fallback,
      cerBefore: origResult.cer, cerAfter: fixedResult.cer,
      werBefore: origResult.wer, werAfter: fixedResult.wer,
      segmentsBefore: segments.length, segmentsAfter: resultSegments.length,
    });
  }

  console.log('\n=== LLM ASR 纠错对比 (SRT 模式) ===');
  console.log('版本\t\tCER 前\tCER 后\t改善\tWER 前\tWER 后\t分段\tFallback');
  for (const r of results) {
    const cerImpr = ((r.cerBefore - r.cerAfter) / Math.max(r.cerBefore, 0.0001) * 100).toFixed(1);
    const werImpr = ((r.werBefore - r.werAfter) / Math.max(r.werBefore, 0.0001) * 100).toFixed(1);
    console.log(
      `${r.label.padEnd(16)}\t${(r.cerBefore * 100).toFixed(2)}%\t${(r.cerAfter * 100).toFixed(2)}%\t${cerImpr}%\t${(r.werBefore * 100).toFixed(2)}%\t${(r.werAfter * 100).toFixed(2)}%\t${r.segmentsAfter}\t${r.fallback ? '⚠' : '✓'}`,
    );
  }

  const summary = results.map(r => ({
    label: r.label, fallback: r.fallback,
    cerBefore: r.cerBefore, cerAfter: r.cerAfter,
    werBefore: r.werBefore, werAfter: r.werAfter,
  }));
  writeFileSync(join(RESULTS_DIR, 'llm-fix-summary.json'), JSON.stringify(summary, null, 2));
  console.log('\nSummary saved to results/llm-fix-summary.json');
}

main().catch(err => {
  console.error('Failed:', err);
  process.exit(1);
});
