import { $ } from "bun";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir } from "@repo/util/file_op";
import { emitLog, nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { OcrFramesResult } from "@repo/subtitle-ocr/types";
import { REPO_ROOT } from "@repo/config/root";

export async function stageAsrOcr(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  startLog(ctx.task.current_stage, ctx.task.id);
  setStage(taskDir, "asr_ocr", {
    last_message: `OCR'ing frames...`,
    progress: 0,
  });

  const frameDir = join(taskDir, "asr_ocr_pre", "frames");
  if (!existsSync(frameDir)) {
    throw new Error(`Frame directory not found: ${frameDir} — run asr_ocr_pre first`);
  }

  const asrOcrArgs = ctx.input.stages.asr_ocr;
  const textScore = asrOcrArgs.text_confidence_threshold;
  const subtitleOnly = asrOcrArgs.subtitleOnly;
  const cleanupFrames = asrOcrArgs.cleanupFrames;

  const ocrBin = join(REPO_ROOT, "target", "release", "subtitle-ocr");
  if (!existsSync(ocrBin)) {
    throw new Error(
      `subtitle-ocr binary not found at ${ocrBin}\n` +
        `Build with: cd ${join(REPO_ROOT, "packages/subtitle-ocr-cli")} && cargo build --release --bin subtitle-ocr`,
    );
  }

  const asrOcrDir = resolve(taskDir, "asr_ocr");
  ensureDir(asrOcrDir);
  const outFile = join(asrOcrDir, "asr_ocr_frames.json");

  const args = [
    "--dir",
    frameDir,
    "--out",
    outFile,
    "--text-score",
    String(textScore),
    ...(subtitleOnly ? ["--subtitle-only"] : []),
  ];

  emitLog(taskDir, `[asr_ocr] binary=${ocrBin} ${args.join(" ")}`);
  const proc = await $`${ocrBin} ${args}`.nothrow();
  if (proc.exitCode !== 0) {
    throw new Error(`subtitle-ocr failed with exit code ${proc.exitCode}: ${proc.stderr}`);
  }

  const result = JSON.parse(readFileSync(outFile, "utf-8")) as OcrFramesResult;
  emitLog(taskDir, `[asr_ocr] ${result.frames.length} frames OCR'd -> ${outFile}`);

  // Cleanup frames (optional)
  if (cleanupFrames) {
    rmSync(frameDir, { recursive: true, force: true });
    emitLog(taskDir, `[asr_ocr] Frames cleaned up`);
  } else {
    emitLog(taskDir, `[asr_ocr] Frames kept at ${frameDir}`);
  }

  setStage(taskDir, "asr_ocr", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
  });
}
