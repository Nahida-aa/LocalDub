import { existsSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { log } from "@repo/util/log";
import { cellOcr } from "./util.ts";

// 关键帧 OCR：消费 sf_ocr_pre 落盘的关键帧（`{start_ms}_{end_ms}.png`），
// 经 subtitle-ocr CLI --dir 批量识别（ms_ms 双时刻 → 每张产出 start/end 两个结果），
// 只写出 `<taskDir>/sf_ocr/frames.json`（OcrFramesResult 原始逐帧结果）。
// 段合并 / 时间调整 / LLM 修正由下游 sf_ocr_fix stage 负责。
export async function stageSfOcr(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  startLog(ctx.task.current_stage, ctx.task.id);
  await setStage(taskDir, "sf_ocr", {
    last_message: "OCR'ing keyframes...",
    progress: 0,
  });

  const preDir = resolve(taskDir, "sf_ocr_pre", "frames");
  if (!existsSync(preDir)) {
    throw new Error(`Keyframe dir not found: ${preDir} — run sf_ocr_pre first`);
  }

  const ocrArgs = ctx.input.stages.sf_ocr;

  const sfOcrDir = resolve(taskDir, "sf_ocr");
  const outFile = join(sfOcrDir, "frames.json");

  const data = await cellOcr(preDir, outFile, ocrArgs);
  if (ocrArgs?.cleanupFrames) {
    rmSync(preDir, { recursive: true, force: true });
    log(`Keyframes cleaned up`);
  }

  await setStage(taskDir, "sf_ocr", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: `OCR'd ${data.frames.length} frame results`,
  });
}
