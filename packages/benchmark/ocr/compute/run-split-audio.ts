import { TaskCtx, writeCtx } from "@repo/core/context/context";
import { stageSplitAudio } from "@repo/core/stages/06_split_audio/split_audio";
import { ensureDir } from "@repo/core/utils/fileOps";
import { join, resolve } from "node:path";
import { existsSync } from "node:fs";

const label = process.argv[2];
if (!label) {
  console.error("Usage: bun run-split-audio.ts <results/label>");
  process.exit(1);
}

const REPO_ROOT = resolve(__dirname, "..", "..", "..", "..");
const resultDir = resolve(__dirname, "..", "results", label);
const metadataDir = join(resultDir, "metadata");
const srtPath = join(metadataDir, "asr_ocr_fix.json");
const audioSource = resolve(REPO_ROOT, "packages", "tmp", "raw-audio.wav");

if (!existsSync(srtPath)) throw new Error(`not found: ${srtPath}`);
if (!existsSync(audioSource)) throw new Error(`not found: ${audioSource}`);

const ctx = {
  task: {
    id: "benchmark-split",
    source: "local",
    url: "",
    status: "running",
    task_dir: resultDir,
    created_at: new Date().toISOString(),
    current_stage: "split_audio",
  },
  pipeline: "dub",
  asr_language: "zh",
  target_language: "zh",
  input: {
    stages: {
      translate: { enabled: false },
      split_audio: { vadAlign: false, vocalsFilePath: audioSource, sourceFilePath: audioSource },
    },
  },
} as unknown as TaskCtx;
writeCtx(ctx);
ensureDir(join(resultDir, "segments", "vocals"));

await stageSplitAudio(ctx);

console.log(`Segments: ${resultDir}/segments/vocals/`);
