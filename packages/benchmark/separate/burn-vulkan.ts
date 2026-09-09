import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import {
  RESULTS_DIR,
  AUDIO_KEYS,
  audioDuration,
  round,
  type BenchmarkResult,
} from "./bench-shared";

const REPO_ROOT = resolve(__dirname, "..", "..", "..");
// demucs-burn 自 vox-lab GitHub Release 下载, 平铺在 data/bin (见 env ensure demucs_burn_wgpu_bin)
const BURN_BIN = join(REPO_ROOT, "data", "bin", "demucs-burn-wgpu");
const REF_DIR = join(REPO_ROOT, "packages", "benchmark", "ref");

function parseStdout(stdout: string, key: string): number {
  const m = stdout.match(new RegExp(`${key}:\\s*([\\d.]+)`));
  return m ? parseFloat(m[1]) : 0;
}

export async function benchmarkBurnVulkan(): Promise<BenchmarkResult[]> {
  const results: BenchmarkResult[] = [];

  if (!existsSync(BURN_BIN)) {
    console.error(
      `[Burn] Binary not found at ${BURN_BIN}. Download with: cargo run -p cli -- env ensure demucs_burn_wgpu_bin`,
    );
    return results;
  }

  console.log("[Burn] Loading model...");
  const loadResult = spawnSync(BURN_BIN, ["--benchmark-load"], { timeout: 120_000 });
  const stdout = loadResult.stdout?.toString() ?? "";
  const loadTimeS = loadResult.status === 0 ? parseStdout(stdout, "Benchmark-Load-Time") : 0;
  console.log(`[Burn] Model loaded in ${loadTimeS.toFixed(3)}s`);

  for (const key of AUDIO_KEYS) {
    const audioPath = join(REF_DIR, `${key}.wav`);
    if (!existsSync(audioPath)) {
      console.warn(`[Burn] ${audioPath} not found, skipping`);
      continue;
    }
    const durationS = audioDuration(audioPath);
    console.log(`[Burn] Processing ${key} (${durationS.toFixed(1)}s)...`);

    const outDir = join(RESULTS_DIR, `burn-${key}-${Date.now()}`);
    mkdirSync(outDir, { recursive: true });

    const result = spawnSync(BURN_BIN, ["--warmup", audioPath, outDir], { timeout: 600_000 });

    if (result.status !== 0) {
      console.error(`[Burn] ${key} failed:`, result.stderr?.toString().slice(-300));
      rmSync(outDir, { recursive: true, force: true });
      continue;
    }

    const resultStdout = result.stdout?.toString() ?? "";
    const genTimeS = parseStdout(resultStdout, "Benchmark-Gen-Time");

    results.push({
      engine: "burn",
      device: "vulkan",
      audioKey: key,
      audioDurationS: round(durationS, 3),
      loadTimeS: round(loadTimeS, 3),
      processTimeS: round(genTimeS, 3),
      totalTimeS: round(loadTimeS + genTimeS, 3),
      rtf: round(genTimeS / durationS, 3),
    });

    console.log(`  RTF: ${results[results.length - 1].rtf}`);
    rmSync(outDir, { recursive: true, force: true });
  }

  return results;
}

async function main() {
  console.log("=== Demucs Burn (Vulkan) Benchmark ===\n");
  const results = await benchmarkBurnVulkan();
  if (results.length === 0) {
    console.error("No results. Build the binary first.");
    process.exit(1);
  }
  console.log("\n--- Results ---");
  console.log("Engine\tDevice\t\tAudio\t\tDur(s)\tLoad(s)\tGen(s)\tRTF");
  for (const r of results) {
    console.log(
      `${r.engine.padEnd(6)}\t${r.device.padEnd(8)}\t${r.audioKey.padEnd(8)}\t${r.audioDurationS.toFixed(1)}\t${r.loadTimeS.toFixed(1)}\t${r.processTimeS.toFixed(1)}\t${r.rtf.toFixed(3)}`,
    );
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
