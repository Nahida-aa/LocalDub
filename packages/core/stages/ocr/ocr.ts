import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { newOcrEngine } from "../../ml/subtitle_ocr/ocr.ts";
import { ensureDir, writeJson } from "@repo/util/file_op";
import {
  emitLog,
  ffmpeg,
  nowISO,
  probeVideoResolution,
  video_source_path,
} from "@repo/core/stages/utils/utils.ts";

import { mergeFrames } from "@repo/subtitle-ocr/ocr_fix/merge_frames";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { srtTime } from "@repo/core/utils/utils";
import { probeVideoDuration } from "../../utils/ffmpeg.ts";
import { FrameResult, OCRRuntime } from "@repo/subtitle-ocr/types";
import { aggregate_boxes } from "@repo/subtitle-ocr/ocr_util";
import { computeBoxYStats } from "@repo/subtitle-ocr/ocr_fix/stats";
import { ocr_segment_adjust } from "@repo/subtitle-ocr/ocr_fix/segment_adjust";

export async function stageOcr(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  await setStage(taskDir, "ocr", {
    last_message: "Extracting frames...",
    progress: 0,
  });

  const videoPath = video_source_path(ctx);
  if (!existsSync(videoPath)) {
    throw new Error(`OCR input not found: ${videoPath}`);
  }

  const ocrCfg = ctx.input?.stages?.ocr;
  const fps = ocrCfg?.fps ?? 2;
  const textScore = ocrCfg?.text_score_threshold ?? 0.45;
  const subtitleOnly = ocrCfg?.subtitleOnly ?? true;
  const runtime = (ocrCfg?.runtime ?? "ort-cpp") as OCRRuntime;
  const device = (ocrCfg?.device ?? "cpu") as
    | "cpu"
    | "cuda"
    | "directml"
    | "coreml"
    | "rocm"
    | "mps";
  const cleanupFrames = ocrCfg?.cleanupFrames ?? false;

  // 1. Extract frames
  const frameDir = join(taskDir, "ocr", "frames");
  ensureDir(frameDir);
  emitLog(taskDir, `[OCR] Extracting frames at ${fps}fps...`);

  const frProbe = spawnSync(
    "ffprobe",
    [
      "-v",
      "error",
      "-select_streams",
      "v:0",
      "-show_entries",
      "stream=r_frame_rate",
      "-of",
      "csv=p=0",
      videoPath,
    ],
    { timeout: 10_000, encoding: "utf-8" },
  );
  const frParts = (frProbe.stdout?.trim() || "30/1").split("/");
  const srcFps = parseInt(frParts[0]) / parseInt(frParts[1]);
  const step = Math.round(srcFps / fps);

  ffmpeg([
    "-y",
    "-i",
    videoPath,
    "-vf",
    `select='not(mod(n,${step}))'`,
    "-vsync",
    "vfr",
    "-qscale:v",
    "2",
    join(frameDir, "frame_%05d.jpg"),
  ]);

  const frameFiles = readdirSync(frameDir)
    .filter((f) => f.endsWith(".jpg"))
    .sort();

  if (!frameFiles.length) {
    throw new Error(`OCR: no frames extracted from ${videoPath}`);
  }

  // 2. OCR each frame
  await setStage(taskDir, "ocr", {
    last_message: `OCR'ing ${frameFiles.length} frames (${runtime})...`,
  });

  const engine = await newOcrEngine(runtime, device);

  const linesArr = await engine.ocrFrames(frameDir, frameFiles, { textScore, subtitleOnly });
  const frameResults: FrameResult[] = [];
  for (let i = 0; i < frameFiles.length; i++) {
    const timestampMs = Math.round(((i * step) / srcFps) * 1000);
    const lines = linesArr[i];
    frameResults.push({ ...aggregate_boxes(lines), timestamp: timestampMs });

    if ((i + 1) % 50 === 0 || i === frameFiles.length - 1) {
      emitLog(taskDir, `[OCR] ${i + 1}/${frameFiles.length} frames`);
    }
  }
  await engine.release();

  // 3. Merge into segments
  const { segments, text } = mergeFrames(frameResults, {
    is_merge_substring: ocrCfg.is_merge_substring,
    dedup_edit_distance: ocrCfg.dedup_edit_distance,
  });
  emitLog(taskDir, `[OCR] ${frameFiles.length} frames → ${segments.length} segments`);

  const { height: videoHeight } = probeVideoResolution(videoPath);

  // 6. Write ocr.json (same format as asr_fix)
  const ocrDir = join(taskDir, "ocr");
  ensureDir(ocrDir);
  const yStats = computeBoxYStats(frameResults);
  const adjustedSegments = ocr_segment_adjust(segments, frameResults, yStats, videoHeight, {
    adjustIsoWeight: ocrCfg?.adjustIsoWeight,
    adjustYWeight: ocrCfg?.adjustYWeight,
    adjustYFactor: ocrCfg?.adjustYFactor,
    isoThresholdMs: ocrCfg?.isoThresholdMs,
  });
  const segmentsOut = adjustedSegments.map((s) => ({
    text: s.text,
    start: s.start_ms,
    end: s.end_ms,
    confidence: s.text_confidence,
    ...(s.y_range ? { y_range: s.y_range } : {}),
    ...(s.frame_count !== undefined ? { frame_count: s.frame_count } : {}),
    ...(s.adjusted_text_confidence !== undefined
      ? { adjusted_text_confidence: s.adjusted_text_confidence }
      : {}),
    ...(s.y_penalty !== undefined ? { y_penalty: s.y_penalty } : {}),
    ...(s.iso_penalty !== undefined ? { isoPenalty: s.iso_penalty } : {}),
  }));
  writeJson(join(ocrDir, "ocr.json"), {
    audio_info: { duration: probeVideoDuration(videoPath) },
    result: { text, segments: segmentsOut },
    _engine: runtime,
    _device: device,
    _fps: fps,
    _textScore: textScore,
    _y_stats: yStats,
    _source: "ocr",
    _frames_raw: frameResults,
  });

  emitLog(taskDir, `[OCR] Written ${segments.length} segs to ocr.json`);

  // 7. Cleanup frames (optional)
  if (cleanupFrames) {
    rmSync(frameDir, { recursive: true, force: true });
    emitLog(taskDir, `[OCR] Frames cleaned up`);
  } else {
    emitLog(taskDir, `[OCR] Frames kept at ${frameDir}`);
  }

  await setStage(taskDir, "ocr", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: `OCR'd ${frameFiles.length} frames → ${segments.length} segments`,
  });
}
