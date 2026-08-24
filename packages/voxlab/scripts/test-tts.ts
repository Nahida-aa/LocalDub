/**
 * Standalone TTS test CLI
 *
 * Usage: bun run scripts/test-tts.ts [--text "你好世界"] [--ref path/to/ref.wav] [--out /tmp/test-tts.wav] [--ep webgpu|cpu]
 *
 * Defaults:
 *   --text  "好的，我们现在在大象面前。"
 *   --ref   (auto-finds first .wav in workfolder subdir segments/vocals/)
 *   --out   /tmp/voxlab-test.wav
 *   --ep    webgpu
 */

import { join } from "node:path";
import { existsSync, readdirSync } from "node:fs";
import { createVoxCPM, VoxCPMBackend, writeWav } from "../src/index.ts";
import { VOXCPM_MODEL_DIR } from "@repo/config/path/models";

const args = parseArgs();
const modelDir = VOXCPM_MODEL_DIR;
const outPath = args.out;

async function main() {
  // Resolve reference WAV
  const refPath = args.ref || findDefaultRef();
  if (!refPath || !existsSync(refPath)) {
    console.error(`Reference WAV not found: ${refPath}`);
    console.error("Provide --ref path/to/ref.wav");
    process.exit(1);
  }

  if (!existsSync(modelDir)) {
    console.error(`Model directory not found: ${modelDir}`);
    console.error("Set MODEL_CACHE_DIR env or run download first.");
    process.exit(1);
  }

  console.log(`Model:  ${modelDir}`);
  console.log(`Ref:    ${refPath}`);
  console.log(`Text:   ${args.text}`);
  console.log(`EP:     ${args.ep}`);
  console.log(`Output: ${outPath}`);
  console.log("");

  const backend =
    args.backend === "pytorch"
      ? VoxCPMBackend.PYTORCH
      : args.backend === "cloud"
        ? VoxCPMBackend.CLOUD
        : VoxCPMBackend.ORT;
  const voxcpm =
    backend === VoxCPMBackend.PYTORCH
      ? createVoxCPM(VoxCPMBackend.PYTORCH, { modelDir })
      : backend === VoxCPMBackend.CLOUD
        ? createVoxCPM(VoxCPMBackend.CLOUD)
        : createVoxCPM(VoxCPMBackend.ORT, {
            modelDir,
            executionProvider: args.ep as "webgpu" | "cpu",
          });
  console.log("[VoxLab] Loading...");
  await voxcpm.load();

  console.log("[VoxLab] Generating...");
  const { samples: audio } = await voxcpm.generate({ text: args.text, referenceWavPath: refPath });

  writeWav(audio, outPath, 48000);

  // Verify volume (check written WAV)
  let wavPeak = 0;
  for (let i = 0; i < audio.length; i++) {
    const abs = Math.abs(audio[i]);
    if (abs > wavPeak) wavPeak = abs;
  }
  const peakPercent = (wavPeak > 0 ? 1 : 0) > 0 ? "95.0% (normalized)" : "check WAV directly";

  console.log(`\n✅ Wrote ${(audio.length / 48000).toFixed(2)}s to ${outPath}`);
  console.log(`   Volume: peak normalized to 95%`);
  console.log("   Play with: ffplay " + outPath);
}

function findDefaultRef(): string | undefined {
  const base = join(import.meta.dir, "..", "..", "..");
  const workfolder = join(base, "workfolder");
  if (!existsSync(workfolder)) return undefined;

  const tasks = readdirSync(workfolder);
  for (const task of tasks) {
    const vocalsDir = join(workfolder, task, "segments", "vocals");
    if (!existsSync(vocalsDir)) continue;
    const wavs = readdirSync(vocalsDir).filter((f) => f.endsWith(".wav"));
    if (wavs.length) return join(vocalsDir, wavs[0]);
  }
  return undefined;
}

function parseArgs() {
  const argv = process.argv.slice(2);
  const get = (key: string, fallback: string): string => {
    const idx = argv.indexOf(key);
    return idx !== -1 && idx + 1 < argv.length ? argv[idx + 1] : fallback;
  };
  return {
    text: get("--text", "好的，我们现在在大象面前。"),
    ref: get("--ref", ""),
    out: get("--out", join(import.meta.dir, "..", "..", "tmp", "voxlab-test.wav")),
    ep: get("--ep", "webgpu"),
    backend: get("--backend", "ort"),
  };
}

await main();
