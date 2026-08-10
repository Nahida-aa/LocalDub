import { existsSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir } from "@repo/util/file_op";
import { emitLog, nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { log } from "@repo/util/log";
import { cellOcr } from "../sf_ocr/util.ts";

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

  const asrOcrDir = resolve(taskDir, "asr_ocr");
  const outFile = join(asrOcrDir, "frames.json");

  await cellOcr(frameDir, outFile, asrOcrArgs);

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
