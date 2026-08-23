import { log } from "@repo/util/log";
import { join, resolve } from "node:path";
import { FrameResult, OcrFramesResult, type OCRRuntime } from "@repo/subtitle-ocr/types";
import {
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
  mkdirSync,
  existsSync,
  readdirSync,
  type WriteFileOptions,
} from "node:fs";
import { ensureDir, writeJson } from "@repo/util/file_op";
import { extract_frames } from "@repo/subtitle-ocr/ffmpeg_util";
import { newOcrEngine } from "@repo/subtitle-ocr/engine";

import { aggregate_boxes } from "@repo/subtitle-ocr/ocr_util";
import { AsrOcrFixArgs } from "./fix_args";

export const hasNearbySameText = (
  rawFrames: FrameResult[],
  i: number,
  f: FrameResult,
  resampleRangeMs = 500,
) => {
  return rawFrames.some(
    (other, j) =>
      j !== i &&
      other.text === f.text &&
      Math.abs(other.timestamp - f.timestamp) <= resampleRangeMs,
  );
};

// 问题: end2fps 抽帧在某些高置信度字幕附近可能留大空隙（相邻同文本帧间距 > RESAMPLE_RANGE_MS），
//       导致后续字幕合并时间边界不准。这里在空隙区间按 RESAMPLE_STEP_MS 步长补抽帧并 OCR，
//       把更多帧并入 rawFrames，提升时间覆盖密度。
// 注意: 仅当某帧"高置信度 + 附近无相同文本帧"才视为孤立点，才触发补抽。
export const collect_resample_candidate_ms = (frames: FrameResult[]) => {
  const RESAMPLE_CONF_THRESH = 0.6; // 仅对高置信度帧补抽，低置信噪声帧不补
  const RESAMPLE_STEP_MS = 100; // 补抽步长
  const RESAMPLE_RANGE_MS = 500; // 在孤立帧 ±500ms 内补抽

  const isolatedInfos: string[] = [];
  const candidateTs = new Set<number>();
  for (const [i, f] of frames.entries()) {
    if (!f.text || f.text_confidence < RESAMPLE_CONF_THRESH) continue;
    // 若附近已有相同文本的帧，说明不是孤立点，无需补抽
    const is_hasNearbySameText = hasNearbySameText(frames, i, f, RESAMPLE_RANGE_MS);
    if (is_hasNearbySameText) continue;
    const prevTs = i > 0 ? frames[i - 1].timestamp : -Infinity;
    const nextTs = i < frames.length - 1 ? frames[i + 1].timestamp : Infinity;
    const gapBefore = f.timestamp - prevTs;
    const gapAfter = nextTs - f.timestamp;
    // 记录孤立点信息用于日志（gapBefore/gapAfter 是该帧与 rawFrames 中相邻帧的时间空隙，仅展示用，不参与孤立判定）
    isolatedInfos.push(
      `  tms=${f.timestamp}ms  text="${f.text.slice(0, 30)}"  conf=${f.text_confidence}  gapBefore=${gapBefore}ms  gapAfter=${gapAfter}ms`,
    );
    // 在孤立帧两侧 ±RESAMPLE_RANGE_MS 内，按步长收集候选时间戳
    for (
      let t = f.timestamp - RESAMPLE_RANGE_MS;
      t <= f.timestamp + RESAMPLE_RANGE_MS;
      t += RESAMPLE_STEP_MS
    ) {
      if (t >= 0) candidateTs.add(t);
    }
  }
  if (isolatedInfos.length > 0) {
    log(
      `[asr_ocr_fix] ${isolatedInfos.length} isolated high-confidence frames:\n${isolatedInfos.join("\n")}`,
    );
  }
  log(`[asr_ocr_fix] Re-sampling ${candidateTs.size} frames at ${RESAMPLE_STEP_MS}ms steps...`);
  return candidateTs;
};

export const resample_candidate_to_ocr_frames = async (
  ocrFramesData: OcrFramesResult,
  candidateTs: Set<number>,
  videoPath: string,
  out_dir: string,
  asrOcrFixArgs: AsrOcrFixArgs,
) => {
  const frames = ocrFramesData.frames;
  // 去掉已存在的时间戳，避免重复抽帧
  const existingTs = new Set(frames.map((f) => f.timestamp));
  const newTs = [...candidateTs].filter((t) => !existingTs.has(t)).sort((a, b) => a - b);

  if (newTs.length > 0) {
    const resampleDir = join(out_dir, "resampled_frames");
    ensureDir(resampleDir);

    // 用 ffmpeg 按时间戳抽帧到 resampled_frames/
    const extracted = extract_frames(newTs, videoPath, resampleDir);

    if (extracted > 0) {
      // 复用原始 OCR 引擎/设备（从 meta 读取）对补抽帧做 OCR
      const runtime = (ocrFramesData.meta?.engine ?? "ort-cpp") as OCRRuntime;
      const device = (ocrFramesData.meta?.device ?? "cpu") as any;

      const frameFiles = readdirSync(resampleDir)
        .filter((f) => f.endsWith(".jpg"))
        .sort();
      const engine = await newOcrEngine(runtime, device);
      const ocrResults = await engine.ocrFrames(resampleDir, frameFiles, {
        textScore: asrOcrFixArgs.adjusted_confidence_threshold,
      });
      await engine.release();

      const newFrames: FrameResult[] = [];
      for (let i = 0; i < frameFiles.length; i++) {
        const tsMatch = frameFiles[i].match(/frame_(\d+)\.jpg/);
        if (!tsMatch) continue;
        const ts = parseInt(tsMatch[1], 10);
        const r = aggregate_boxes(ocrResults[i]);
        if (r.text) newFrames.push({ ...r, timestamp: ts });
      }

      if (newFrames.length > 0) {
        const before = frames.length;
        frames.push(...newFrames);
        frames.sort((a, b) => a.timestamp - b.timestamp);
        log(`[asr_ocr_fix] Added ${newFrames.length} OCR frames (${before} → ${frames.length})`);

        // 写回 asr_ocr_fix/ocr_frames.json（注意: 不是 asr_ocr/ocr_frames.json，
        // 原始逐帧结果保持不动；这里是有重采样补帧的版本）
        writeJson(join(out_dir, "ocr_frames.json"), { frames: frames, meta: ocrFramesData.meta });
      }
    }
  }
  return frames;
};

/**
 * 问题: end2fps 抽帧在某些高置信度字幕附近可能留大空隙（相邻同文本帧间距 > RESAMPLE_RANGE_MS），
 *       导致后续字幕合并时间边界不准。这里在空隙区间按 RESAMPLE_STEP_MS 步长补抽帧并 OCR，
 *       把更多帧并入 rawFrames，提升时间覆盖密度。
 * 注意: 仅当某帧"高置信度 + 附近无相同文本帧"才视为孤立点，才触发补抽
 */
export const resample_to_ocr_frames = async (
  ocrFramesData: OcrFramesResult,
  videoPath: string,
  out_dir: string,
  asrOcrFixArgs: AsrOcrFixArgs,
) => {
  const candidateTs = collect_resample_candidate_ms(ocrFramesData.frames);
  return await resample_candidate_to_ocr_frames(
    ocrFramesData,
    candidateTs,
    videoPath,
    out_dir,
    asrOcrFixArgs,
  );
};
