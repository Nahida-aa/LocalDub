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
const SCRIPT = join(REPO_ROOT, 'packages', 'cli', 'scripts', 'separate', 'run.py');

const isWin = process.platform === 'win32';
const PYTHON_BIN = join(REPO_ROOT, '.venv', isWin ? 'Scripts' : 'bin', isWin ? 'python.exe' : 'python');

function measureLoadTime(device: string, shifts: number = 3): number {
  console.log(`[PyTorch] Measuring model load time (shifts=${shifts})...`);
  const t0 = performance.now();
  const result = spawnSync(PYTHON_BIN, [SCRIPT, '--benchmark-load', '--device', device, '--shifts', String(shifts)], {
    timeout: 600_000,
  });
  if (result.status !== 0) {
    throw new Error(`--benchmark-load failed: ${result.stderr?.toString().slice(-300)}`);
  }
  return (performance.now() - t0) / 1000;
}

export async function benchmarkPyTorch(shifts: number = 3): Promise<BenchmarkResult[]> {
  const results: BenchmarkResult[] = [];
  const device = 'cpu';

  const loadTimeS = round(measureLoadTime(device, shifts), 3);
  console.log(`[PyTorch] Model loaded in ${loadTimeS}s (shifts=${shifts})`);

  for (const key of AUDIO_KEYS) {
    const audioPath = join(REF_DIR, `${key}.wav`);
    if (!existsSync(audioPath)) {
      console.warn(`[PyTorch] ${audioPath} not found, skipping`);
      continue;
    }
    const durationS = audioDuration(audioPath);
    console.log(`[PyTorch] Processing ${key} (${durationS.toFixed(1)}s)...`);

    const outDir = join(RESULTS_DIR, `pytorch-${key}-${Date.now()}`);
    mkdirSync(outDir, { recursive: true });

    const t1 = performance.now();
    const args = [SCRIPT, audioPath, outDir, '--device', device, '--shifts', String(shifts)];
    const timeout = key === 'long' ? 3_600_000 : 600_000;
    const result = spawnSync(PYTHON_BIN, args, { timeout });

    if (result.status !== 0) {
      console.error(`[PyTorch] ${key} failed:`, result.stderr?.toString().slice(-300));
      rmSync(outDir, { recursive: true, force: true });
      continue;
    }

    const totalTimeS = (performance.now() - t1) / 1000;
    const processTimeS = totalTimeS - loadTimeS;

    results.push({
      engine: 'pytorch',
      device,
      audioKey: key,
      audioDurationS: round(durationS, 3),
      loadTimeS: round(loadTimeS, 3),
      processTimeS: round(processTimeS, 3),
      totalTimeS: round(totalTimeS, 3),
      rtf: round(processTimeS / durationS, 3),
    });

    console.log(`  RTF: ${results[results.length - 1].rtf}`);

    rmSync(outDir, { recursive: true, force: true });
  }

  return results;
}
