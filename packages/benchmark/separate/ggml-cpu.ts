import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import {
  RESULTS_DIR,
  REF_DIR,
  AUDIO_KEYS,
  audioDuration,
  round,
  type BenchmarkResult,
} from './bench-shared';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const BIN = join(REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main');
const BIN_ST = join(REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs.cpp.main');
const GGML_MODEL = join(REPO_ROOT, 'packages', 'tmp', 'demucs-ggml', 'ggml-model-htdemucs-4s-f16.bin');

// Physical cores: match Eigen recommendation (OMP_NUM_THREADS = physical cores)
// MT strategy: 4 std::thread × 2 OMP threads = 8 (matches 8c Ryzen 7 H 255)
const NUM_PHYSICAL_CORES = 8;
const NUM_MT_THREADS = 4;
const OMP_THREADS = Math.floor(NUM_PHYSICAL_CORES / NUM_MT_THREADS);

const ENV_OMP = { ...process.env, OMP_NUM_THREADS: String(OMP_THREADS) };

function parseLoadTime(stdout: string): number {
  const m = stdout.match(/Loaded model .+ in ([\d.]+) s/);
  return m ? parseFloat(m[1]) : 0;
}

export async function benchmarkGGML(): Promise<BenchmarkResult[]> {
  const results: BenchmarkResult[] = [];

  // Build with OpenBLAS; prefer MT binary if available
  const bin = existsSync(BIN) ? BIN : BIN_ST;

  console.log('[GGML] Loading model...');
  const loadArgs = existsSync(BIN)
    ? [GGML_MODEL, join(REF_DIR, 'short.wav'), '/tmp/demucs-ggml-loadtest/', String(NUM_MT_THREADS)]
    : [GGML_MODEL, join(REF_DIR, 'short.wav'), '/tmp/demucs-ggml-loadtest/'];
  const t0 = performance.now();
  const loadResult = spawnSync(bin, loadArgs, { timeout: 120_000, env: ENV_OMP });
  const stdout = loadResult.stdout?.toString() ?? '';
  const loadTimeS = loadResult.status === 0
    ? parseLoadTime(stdout)
    : (performance.now() - t0) / 1000;
  rmSync('/tmp/demucs-ggml-loadtest/', { recursive: true, force: true });
  console.log(`[GGML] Model loaded in ${loadTimeS.toFixed(3)}s`);

  for (const key of AUDIO_KEYS) {
    const audioPath = join(REF_DIR, `${key}.wav`);
    if (!existsSync(audioPath)) {
      console.warn(`[GGML] ${audioPath} not found, skipping`);
      continue;
    }
    const durationS = audioDuration(audioPath);
    console.log(`[GGML] Processing ${key} (${durationS.toFixed(1)}s)...`);

    const outDir = join(RESULTS_DIR, `ggml-${key}-${Date.now()}`);
    mkdirSync(outDir, { recursive: true });

    const runArgs = existsSync(BIN)
      ? [GGML_MODEL, audioPath, outDir, String(NUM_MT_THREADS)]
      : [GGML_MODEL, audioPath, outDir];

    const t1 = performance.now();
    const result = spawnSync(bin, runArgs, { timeout: 600_000, env: ENV_OMP });

    if (result.status !== 0) {
      console.error(`[GGML] ${key} failed:`, result.stderr?.toString().slice(-300));
      rmSync(outDir, { recursive: true, force: true });
      continue;
    }

    const totalTimeS = (performance.now() - t1) / 1000;
    const processTimeS = round(totalTimeS - loadTimeS, 3);

    results.push({
      engine: 'ggml',
      device: 'cpu',
      audioKey: key,
      audioDurationS: round(durationS, 3),
      loadTimeS: round(loadTimeS, 3),
      processTimeS,
      totalTimeS: round(totalTimeS, 3),
      rtf: round(processTimeS / durationS, 3),
    });

    console.log(`  RTF: ${results[results.length - 1].rtf}`);
    rmSync(outDir, { recursive: true, force: true });
  }

  return results;
}
