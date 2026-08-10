import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { newOcrEngine } from "../../ml/subtitle_ocr/ocr.ts";
import { ensureDir, writeJson } from "@repo/util/file_op";
import { emitLog, nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { computeBoxYStats } from "@repo/subtitle-ocr/ocr_fix/stats";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { probeVideoDuration } from "../../utils/ffmpeg.ts";
import { FrameResult, OcrFramesResult } from "@repo/subtitle-ocr/types";
import { aggregate_boxes } from "@repo/subtitle-ocr/ocr_util";
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

  const asrOcrCfg = ctx.input?.stages?.asr_ocr;
  const textScore = asrOcrCfg?.text_confidence_threshold ?? 0.45;
  const subtitleOnly = asrOcrCfg?.subtitleOnly ?? true;
  const runtime = asrOcrCfg?.runtime ?? "ort-cpp";
  const device = asrOcrCfg?.device ?? "cpu";
  const cleanupFrames = asrOcrCfg?.cleanupFrames ?? false;

  // OCR each frame
  const engine = await newOcrEngine(runtime, device);

  const frameFiles = readdirSync(frameDir)
    .filter((f) => f.endsWith(".jpg"))
    .sort();
  const linesArr = await engine.ocrFrames(frameDir, frameFiles, { textScore, subtitleOnly });
  await engine.release();
  const frameResults: FrameResult[] = [];

  for (let i = 0; i < frameFiles.length; i++) {
    const tsMatch = frameFiles[i].match(/(\d+)\.jpg/);
    const timestampMs = tsMatch ? parseInt(tsMatch[1]) : 0;
    const lines = linesArr[i];
    const r = aggregate_boxes(lines);
    frameResults.push({ ...r, timestamp: timestampMs });

    if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
      log(` ${i + 1}/${frameFiles.length} frames`);
    }
  }

  const asrOcrDir = resolve(taskDir, "asr_ocr");
  ensureDir(asrOcrDir);

  // Write ocr_frames.json — raw frame data for debugging/reproducibility
  const ocrFramesFile: OcrFramesResult = {
    frames: frameResults,
    meta: {
      engine: runtime,
      device: device,
    },
  };
  writeJson(join(asrOcrDir, "ocr_frames.json"), ocrFramesFile);

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
