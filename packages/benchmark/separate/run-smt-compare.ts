import { spawnSync } from 'node:child_process';
import { existsSync, copyFileSync, readdirSync, unlinkSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { RESULTS_DIR, round, audioDuration } from './bench-shared';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const VIDEO_PATH = join(REPO_ROOT, 'packages', 'benchmark', 'video_source.mp4');
const TMP_AUDIO = join(REPO_ROOT, 'packages', 'tmp', 'demucs-smt-compare.wav');
const GGML_BIN = join(REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main');
const GGML_MODEL = join(REPO_ROOT, 'packages', 'tmp', 'demucs-ggml', 'ggml-model-htdemucs-4s-f16.bin');

interface SMTConfig {
  label: string;
  numThreads: string;
  ompThreads: string;
  totalThreads: string;
}

const CONFIGS: SMTConfig[] = [
  { label: '8t-phys', numThreads: '4', ompThreads: '2', totalThreads: '8 (物理核)' },
  { label: '16t-smt', numThreads: '4', ompThreads: '4', totalThreads: '16 (含 SMT)' },
];

function extractAudio(): number {
  console.log('[SMT] Extracting audio...');
  const r = spawnSync('ffmpeg', [
    '-y', '-i', VIDEO_PATH, '-vn', '-ac', '2', '-ar', '44100', TMP_AUDIO,
  ], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`ffmpeg failed: ${r.stderr?.toString().slice(-200)}`);
  const dur = audioDuration(TMP_AUDIO);
  console.log(`  Duration: ${dur.toFixed(1)}s`);
  return dur;
}

interface RunResult {
  wallS: number;
  loadS: number;
  rtf: number;
}

function runGGML(cfg: SMTConfig, durS: number): RunResult {
  console.log(`\n[SMT] ${cfg.label} (mt=${cfg.numThreads}, OMP=${cfg.ompThreads}, 总=${cfg.totalThreads})...`);

  const outDir = join(RESULTS_DIR, `smt-${cfg.label}`);
  spawnSync('rm', ['-rf', outDir]);
  const t0 = performance.now();

  const r = spawnSync(GGML_BIN, [GGML_MODEL, TMP_AUDIO, outDir, cfg.numThreads], {
    timeout: 600_000,
    env: { ...process.env, OMP_NUM_THREADS: cfg.ompThreads },
  });

  if (r.status !== 0) throw new Error(`${cfg.label} failed: ${r.stderr?.toString().slice(-300)}`);
  const wallS = (performance.now() - t0) / 1000;

  // Copy stems to results with label
  const outFiles = readdirSync(outDir).filter(f => f.endsWith('.wav'));
  for (const f of outFiles) {
    const src = join(outDir, f);
    const dst = join(RESULTS_DIR, f.replace('target_', `smt-${cfg.label}-stem`));
    copyFileSync(src, dst);
    console.log(`  Saved: ${dst}`);
  }

  // Parse loadTime from stderr (GGML prints "Loaded model in X.XXs")
  const stderr = r.stderr?.toString() || '';
  const loadMatch = stderr.match(/Loaded model in ([\d.]+)s/);
  const loadS = loadMatch ? parseFloat(loadMatch[1]) : 0;

  const procS = wallS - loadS;
  const rtf = round(procS / durS, 3);
  console.log(`  wall=${wallS.toFixed(1)}s load=${loadS.toFixed(1)}s proc=${procS.toFixed(1)}s RTF=${rtf}`);

  return { wallS, loadS, rtf };
}

interface SMTResult {
  label: string;
  config: string;
  totalThreads: string;
  wallS: number;
  loadS: number;
  procS: number;
  rtf: number;
}

async function main() {
  if (!existsSync(VIDEO_PATH)) {
    console.error(`Video not found: ${VIDEO_PATH}`);
    process.exit(1);
  }

  const durS = extractAudio();
  const results: SMTResult[] = [];

  for (const cfg of CONFIGS) {
    try {
      const { wallS, loadS, rtf } = runGGML(cfg, durS);
      results.push({
        label: cfg.label,
        config: `mt=${cfg.numThreads}, OMP=${cfg.ompThreads}`,
        totalThreads: cfg.totalThreads,
        wallS, loadS,
        procS: wallS - loadS,
        rtf,
      });
    } catch (err) {
      console.error(`  FAILED: ${err}`);
    }
  }

  console.log('\n=== SMT 对比结果（video_source.mp4, GGML）===');
  console.log('配置\t\t总线程\tWall(s)\tLoad(s)\tProc(s)\tRTF\tSpeedup');
  const ref = results.length > 0 ? results[0].wallS : 0;
  for (const r of results) {
    const speedup = ref > 0 ? (ref / r.wallS).toFixed(3) : '-';
    console.log(
      `${r.config.padEnd(16)}\t${r.totalThreads.padEnd(10)}\t${r.wallS.toFixed(1)}\t${r.loadS.toFixed(1)}\t${r.procS.toFixed(1)}\t${r.rtf.toFixed(3)}\t${speedup}`,
    );
  }

  // Cleanup
  if (existsSync(TMP_AUDIO)) unlinkSync(TMP_AUDIO);
}

main().catch((err) => {
  console.error('Failed:', err);
  process.exit(1);
});
