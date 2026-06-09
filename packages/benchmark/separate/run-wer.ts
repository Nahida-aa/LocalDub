import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const RESULTS_DIR = join(__dirname, 'results');
const WHISPER_BUILD = join(REPO_ROOT, 'submodule', 'whisper.cpp', 'build');
const WHISPER_CLI = join(WHISPER_BUILD, 'bin', 'whisper-cli');
const MODEL = process.env.WHISPER_MODEL || join(process.env.HOME || '/root', '.cache', 'pywhispercpp', 'ggml-large-v3-turbo.bin');
const PYTHON_BIN = join(REPO_ROOT, '.venv', 'bin', 'python');
const WER_PY = join(__dirname, 'wer.py');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr_manual.json');

interface VocalsFile {
  label: string;
  path: string;
}

const VOCALS: VocalsFile[] = [
  { label: 'ggml-shifts1', path: join(RESULTS_DIR, 'video-ggml-shifts1-vocals.wav') },
  { label: 'pytorch-shifts1', path: join(RESULTS_DIR, 'video-pytorch-shifts1-vocals.wav') },
  { label: 'pytorch-shifts3', path: join(RESULTS_DIR, 'video-pytorch-shifts3-vocals.wav') },
];

function parseWhisperSegments(stdout: string): { text: string; segments: any[] } {
  const segments: any[] = [];
  const lines = stdout.split('\n');
  for (const line of lines) {
    const m = line.match(/^\[(\d+):(\d+):(\d+)\.(\d+)\s*-->\s*(\d+):(\d+):(\d+)\.(\d+)\]\s+(.*)/);
    if (m) {
      const startS = parseInt(m[1]) * 3600 + parseInt(m[2]) * 60 + parseInt(m[3]) + parseInt(m[4]) / 100;
      const endS = parseInt(m[5]) * 3600 + parseInt(m[6]) * 60 + parseInt(m[7]) + parseInt(m[8]) / 100;
      const text = m[9].trim();
      segments.push({ text, start: startS, end: endS });
    }
  }
  const fullText = segments.map(s => s.text).join(' ');
  return { text: fullText, segments };
}

function transcribe(label: string, wavPath: string): { text: string; segments: any[]; wallS: number } {
  console.log(`[WER] Transcribing ${label}...`);
  const t0 = performance.now();
    const result = spawnSync(WHISPER_CLI, ['-m', MODEL, wavPath, '-l', 'zh', '-t', '4', '--print-progress', '-ng'], {
    timeout: 600_000,
    env: {
      ...process.env,
      LD_LIBRARY_PATH: [
        join(WHISPER_BUILD, 'src'),
        join(WHISPER_BUILD, 'ggml', 'src'),
        join(WHISPER_BUILD, 'ggml', 'src', 'ggml-hip'),
        process.env.LD_LIBRARY_PATH || '',
      ].filter(Boolean).join(':'),
    },
  });
  const wallS = (performance.now() - t0) / 1000;

  if (result.status !== 0) {
    throw new Error(`whisper-cli failed for ${label}: ${result.stderr?.toString().slice(-300)}`);
  }

  const { text, segments } = parseWhisperSegments(result.stdout.toString());
  return { text, segments, wallS };
}

function computeWER(hypFile: string): { wer: number; cer: number } {
  const r = spawnSync(PYTHON_BIN, [WER_PY, GROUND_TRUTH, hypFile], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`wer.py failed: ${r.stderr?.toString().slice(-200)}`);
  return JSON.parse(r.stdout.toString());
}

interface WERResult {
  label: string;
  wallS: number;
  refWords: number;
  hypWords: number;
  wer: number;
  cer: number;
}

async function main() {
  mkdirSync(RESULTS_DIR, { recursive: true });
  const results: WERResult[] = [];

  for (const { label, path } of VOCALS) {
    if (!existsSync(path)) {
      console.warn(`  SKIP: ${path} not found`);
      continue;
    }

    const { text, segments, wallS } = transcribe(label, path);

    // Save hypothesis ASR JSON
    const hypFile = join(RESULTS_DIR, `wer-${label}.json`);
    writeFileSync(hypFile, JSON.stringify({
      audio_info: { duration: 0 },
      result: { text, segments },
      _device: 'gpu',
    }, null, 2));

    const { wer, cer, ref_words, hyp_words } = computeWER(hypFile);
    results.push({ label, wallS, refWords: ref_words, hypWords: hyp_words, wer, cer });
  }

  console.log('\n=== WER 对比 ===');
  console.log('Backend\t\tWall(s)\t\tRefWords\tHypWords\tWER\t\tCER');
  for (const r of results) {
    console.log(
      `${r.label.padEnd(16)}\t${r.wallS.toFixed(1)}s\t\t${r.refWords}\t\t${r.hypWords}\t\t${(r.wer * 100).toFixed(2)}%\t\t${(r.cer * 100).toFixed(2)}%`,
    );
  }

  // Save summary
  const summaryPath = join(RESULTS_DIR, 'wer-summary.json');
  writeFileSync(summaryPath, JSON.stringify(results, null, 2));
  console.log(`\nSummary saved to ${summaryPath}`);
}

main().catch(err => {
  console.error('Failed:', err);
  process.exit(1);
});
