import { $, spawn } from "bun";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir } from "@repo/util/file_op";
import { emitLog, nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { OcrFramesResult } from "@repo/subtitle-ocr/types";
import { REPO_ROOT } from "@repo/config/root";
import { log } from "@repo/util/log";

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

  const ocrBin = join(REPO_ROOT, "target", "release", "subtitle-ocr");
  if (!existsSync(ocrBin)) {
    log(`[asr_ocr] subtitle-ocr 未构建，自动编译...`);
    const build = await $`cargo build --release -p subtitle-ocr-cli --bin subtitle-ocr`
      .cwd(REPO_ROOT)
      .nothrow();
    if (build.exitCode !== 0) {
      throw new Error(`subtitle-ocr 编译失败 (exit ${build.exitCode}):\n${build.stderr}`);
    }
  }

  const asrOcrDir = resolve(taskDir, "asr_ocr");
  ensureDir(asrOcrDir);
  const outFile = join(asrOcrDir, "frames.json");

  const args = [
    "--dir",
    frameDir,
    "--out",
    outFile,
    "--text-confidence-threshold",
    String(asrOcrArgs.text_confidence_threshold),
    ...(asrOcrArgs.subtitleOnly ? ["--subtitle-only"] : []),
  ];

  log(`binary=${ocrBin} ${args.join(" ")}`);
  const proc = spawn([ocrBin, ...args], {
    cwd: REPO_ROOT,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    throw new Error(`subtitle-ocr failed with exit code ${exitCode}`);
  }

  const result = JSON.parse(readFileSync(outFile, "utf-8")) as OcrFramesResult;
  log(`${result.frames.length} frames OCR'd -> ${outFile}`);

  // Cleanup frames (optional)
  if (asrOcrArgs.cleanupFrames) {
    rmSync(frameDir, { recursive: true, force: true });
    log(`Frames cleaned up`);
  } else {
    log(`Frames kept at ${frameDir}`);
  }

  setStage(taskDir, "asr_ocr", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
  });
}
