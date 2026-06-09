import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, copyFileSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { RESULTS_DIR, round, audioDuration } from './bench-shared';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const VIDEO_PATH = join(REPO_ROOT, 'packages', 'benchmark', 'video_source.mp4');
const TMP_AUDIO = join(REPO_ROOT, 'packages', 'tmp', 'demucs-video-bench.wav');
const PYTHON_BIN = join(REPO_ROOT, '.venv', 'bin', 'python');
const PYTORCH_SCRIPT = join(REPO_ROOT, 'packages', 'cli', 'scripts', 'separate', 'run.py');
const GGML_BIN = join(REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main');
const GGML_MODEL = join(REPO_ROOT, 'packages', 'tmp', 'demucs-ggml', 'ggml-model-htdemucs-4s-f16.bin');
const ORT_MODULE = join(REPO_ROOT, 'packages', 'cli', 'src', 'ml', 'demucs', 'demucs.ts');

function extractAudio(): number {
  console.log('[video] Extracting audio...');
  const t0 = performance.now();
  const r = spawnSync('ffmpeg', [
    '-y', '-i', VIDEO_PATH, '-vn', '-ac', '2', '-ar', '44100', TMP_AUDIO,
  ], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`ffmpeg failed: ${r.stderr?.toString().slice(-200)}`);
  const dur = audioDuration(TMP_AUDIO);
  console.log(`  Duration: ${dur.toFixed(1)}s (${(performance.now() - t0) / 1000}s)`);
  return dur;
}

function runPyTorch(durS: number, shifts: number, label: string) {
  console.log(`\n[PyTorch shifts=${shifts}] ${label}...`);
  const t0 = performance.now();
  const outDir = join(RESULTS_DIR, `video-pytorch-s${shifts}`);
  mkdirSync(outDir, { recursive: true });

  const r = spawnSync(PYTHON_BIN, [
    PYTORCH_SCRIPT, TMP_AUDIO, outDir, '--device', 'cpu', '--shifts', String(shifts),
  ], { timeout: 3_600_000 });

  if (r.status !== 0) {
    console.error(`  FAILED: ${r.stderr?.toString().slice(-300)}`);
    return;
  }

  const wallS = (performance.now() - t0) / 1000;
  const src = join(outDir, 'media', 'audio_vocals.wav');
  if (existsSync(src)) {
    const dst = join(RESULTS_DIR, `video-pytorch-shifts${shifts}-vocals.wav`);
    copyFileSync(src, dst);
    console.log(`  WAV saved: ${dst}`);
  }
  console.log(`  wall=${wallS.toFixed(1)}s RTF=${round(wallS / durS, 3)}`);
}

function runGGML(durS: number, label: string) {
  console.log(`\n[GGML shift=1] ${label}...`);
  const t0 = performance.now();
  const outDir = join(RESULTS_DIR, `video-ggml-s1`);
  mkdirSync(outDir, { recursive: true });

  const r = spawnSync(GGML_BIN, [GGML_MODEL, TMP_AUDIO, outDir, '4'], {
    timeout: 600_000,
    env: { ...process.env, OMP_NUM_THREADS: '2' },
  });

  if (r.status !== 0) {
    console.error(`  FAILED: ${r.stderr?.toString().slice(-300)}`);
    return;
  }

  const wallS = (performance.now() - t0) / 1000;
  const src = join(outDir, 'target_3_vocals.wav');
  if (existsSync(src)) {
    const dst = join(RESULTS_DIR, `video-ggml-shifts1-vocals.wav`);
    copyFileSync(src, dst);
    console.log(`  WAV saved: ${dst}`);
  }
  console.log(`  wall=${wallS.toFixed(1)}s RTF=${round(wallS / durS, 3)}`);
}

function runORT(durS: number, label: string) {
  console.log(`\n[ORT no-shift] ${label}...`);
  console.log('  (requires ts-node/bun runner; skipping for now)');
  // ORT is in-process TS; would need to run separately via bun.
  // For now: skip and note user can run onnx-cpu.ts manually.
}

async function main() {
  if (!existsSync(VIDEO_PATH)) {
    console.error(`Video not found: ${VIDEO_PATH}`);
    process.exit(1);
  }

  mkdirSync(RESULTS_DIR, { recursive: true });
  const durS = extractAudio();

  console.log(`\n=== shifts=1 (公平对比) ===`);
  runPyTorch(durS, 1, 'shifts=1');
  runGGML(durS, 'shift=1');
  runORT(durS, 'no shift');

  console.log(`\n=== shifts=3 (高质量) ===`);
  runPyTorch(durS, 3, 'shifts=3');

  console.log(`\n=== 结果文件 ===`);
  for (const f of readdirSync(RESULTS_DIR)) {
    if (f.includes('video-') && f.endsWith('.wav')) {
      const size = readFileSync(join(RESULTS_DIR, f)).length;
      console.log(`  ${f} (${(size / 1024 / 1024).toFixed(1)} MB)`);
    }
  }
}

main().catch((err) => {
  console.error('Failed:', err);
  process.exit(1);
});
