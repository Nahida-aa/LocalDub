import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { newOcrEngine, type OCRRuntime } from "../../ml/subtitle_ocr/ocr.ts";
import { ensureDir, writeJson, readJson } from "@repo/core/utils/fileOps";
import { emitLog, nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { aggregate_boxes, computeBoxYStats } from "../ocr/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { Segment, SegmentWithAdjusted } from "@repo/core/ml/subtitle_ocr/types";
import { probeVideoDuration } from "../../utils/ffmpeg.ts";
import { FrameResult } from "@repo/subtitle-ocr/types";

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
  const textScore = asrOcrCfg?.textScore ?? 0.45;
  const subtitleOnly = asrOcrCfg?.subtitleOnly ?? true;
  const runtime = (asrOcrCfg?.runtime ?? "ort-cpp") as OCRRuntime;
  const device = (asrOcrCfg?.device ?? "cpu") as
    | "cpu"
    | "cuda"
    | "directml"
    | "coreml"
    | "rocm"
    | "mps";
  const cleanupFrames = asrOcrCfg?.cleanupFrames ?? false;

  // OCR each frame
  const engine = await newOcrEngine(runtime, device);

  const frameFiles = readdirSync(frameDir)
    .filter((f) => f.endsWith(".jpg"))
    .sort();
  const linesArr = await engine.ocrFrames(frameDir, frameFiles, { textScore, subtitleOnly });
  const frameResults: FrameResult[] = [];

  for (let i = 0; i < frameFiles.length; i++) {
    const tsMatch = frameFiles[i].match(/frame_(\d+)\.jpg/);
    const timestampMs = tsMatch ? parseInt(tsMatch[1]) : 0;
    const lines = linesArr[i];
    const r = aggregate_boxes(lines);
    frameResults.push({ ...r, timestamp: timestampMs });

    if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
      emitLog(taskDir, `[asr_ocr] ${i + 1}/${frameFiles.length} frames`);
    }
  }
  await engine.release();

  const asrOcrDir = resolve(taskDir, "asr_ocr");
  ensureDir(asrOcrDir, ctx);

  // Write ocr_frames.json — raw frame data for debugging/reproducibility
  writeJson(
    join(asrOcrDir, "ocr_frames.json"),
    {
      _frames_raw: frameResults,
      audio_info: { duration: probeVideoDuration(video_source_path(ctx)) },
      _source: "asr_ocr",
      _engine: runtime,
      _device: device,
    },
    ctx,
  );

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
