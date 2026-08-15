import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { readJson } from "@repo/core/utils/fileOps";
import { nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { fixOverlap } from "@repo/subtitle-ocr/ocr_fix/merge_frames";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { OcrFramesResult, OcrSegment } from "@repo/subtitle-ocr/types";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { resample_to_ocr_frames } from "@repo/subtitle-ocr/ocr_fix/resample";

import { to } from "@repo/shared/lib/utils/try";
import { log } from "@repo/util/log";
import { OcrFramesBoxFilteredResult } from "@repo/subtitle-ocr/ocr_fix/box_filter";
import { AsrSplitResult } from "./ocr_pre.ts";
import { ocrLlmFix } from "../sf_ocr/llm_fix.ts";
import { AsrResult } from "@repo/subtitle-asr/types";
import { cellOcrPost } from "../sf_ocr/util.ts";

export async function stageAsrOcrFix(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  const args = ctx.input?.stages?.asr_ocr_fix;
  await setStage(taskDir, "asr_ocr_fix", {
    last_message: "Fusing ASR + OCR...",
    progress: 0,
  });

  const outDir = resolve(taskDir, "asr_ocr_fix");

  // === 读取上游产物 ===
  // asr.json          : ASR 原始分段（whisper 等）
  // asr_split.json    : asr_ocr_pre 按标点/词切分后的 ASR 段（驱动抽帧时间戳）
  // ocr_frames.json   : asr_ocr 阶段产出的逐帧 OCR 原始结果（OcrFramesResult 格式）
  const asrFile = join(taskDir, "asr", "asr.json");
  const ocrDir = join(taskDir, "asr_ocr");

  const asrSplitFile = join(taskDir, "asr_ocr_pre", "asr_split.json");
  const ocrFramesFile = join(ocrDir, "frames.json");

  if (!existsSync(asrFile)) throw new Error(`asr.json not found: ${asrFile}`);
  if (!existsSync(asrSplitFile)) throw new Error(`asr_split.json not found, run asr_ocr_pre first`);
  if (!existsSync(ocrFramesFile)) throw new Error(`frames.json not found, run asr_ocr first`);

  ensureDir(outDir);

  const asrRawLen = (await readJson<AsrResult>(asrFile)).result.segments.length;
  const asrSplitData = await readJson<AsrSplitResult>(asrSplitFile);
  const ocrFramesData = await readJson<OcrFramesResult>(ocrFramesFile);

  // ASR 切分后的段（只取 text/start/end，用于后续时间边界对齐）
  const asrSegs = asrSplitData.result.segments;

  const asrOcrFixArg = ctx.input.stages.asr_ocr_fix;

  const videoPath = video_source_path(ctx);

  asrOcrFixArg.is_resample
    ? await resample_to_ocr_frames(ocrFramesData, videoPath, outDir, asrOcrFixArg) // === 阶段 1: 重采样补充帧 ===
    : ocrFramesData.frames; // 逐帧 OCR 原始结果；后续的重采样/过滤都作用在 rawFrames 上
  const framesFile = asrOcrFixArg.is_resample ? join(outDir, "frames.json") : ocrFramesFile;

  const videoFile = video_source_path(ctx);
  // === 阶段 5: 按 adjusted_confidence_threshold 过滤低置信段 ===
  // ocr_filtered.json：以 adjustedSegs 为输入，按 adjustedConfidence 过滤（Y 偏移 + 孤立惩罚）
  const filteredResult = await cellOcrPost(framesFile, videoFile, outDir, args);
  const cleanFramesResult = await readJson<OcrFramesBoxFilteredResult>(
    join(outDir, "frames_box_filter.json"),
  );

  // === 阶段 6: 时间边界对齐到 ASR ===
  // asr_ocr_merged.json：复用 ocr_filtered.json 的 segments，时间边界对齐到 ASR
  // 对每个 OCR segment，找到时间上最重叠的 ASR segment，用 ASR 的 start/end 作为新边界
  const asrOcrSegs: OcrSegment[] = filteredResult.result.segments.map((seg) => {
    let bestAsr: OcrSegment | undefined = undefined;
    let bestOverlap = 0;
    for (const asr of asrSegs) {
      let overlap: number;
      if (seg.start_ms === seg.end_ms) {
        overlap = seg.start_ms >= asr.start_ms && seg.start_ms <= asr.end_ms ? 1 : 0;
      } else {
        overlap = Math.max(
          0,
          Math.min(seg.end_ms, asr.end_ms) - Math.max(seg.start_ms, asr.start_ms),
        );
      }
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        bestAsr = asr;
      }
    }
    return {
      text: seg.text,
      start_ms: bestAsr ? bestAsr.start_ms : seg.start_ms,
      end_ms: bestAsr ? bestAsr.end_ms : seg.end_ms,
      text_confidence: seg.text_confidence,
      y_range: seg.y_range,
    } as OcrSegment;
  });

  const asrOcrText = asrOcrSegs.map((s) => s.text).join(" ");

  writeJson(join(outDir, "asr_ocr_merged.json"), {
    audio_info: { duration: asrOcrSegs.length > 0 ? asrOcrSegs[asrOcrSegs.length - 1].end_ms : 0 },
    _engine: "asr_ocr",
    _fusion_params: {
      strategy: "end2fps",
      ocrCalls: asrOcrSegs.length,
      asrSegs: asrRawLen,
      asrSplits: asrSegs.length,
    },
    result: {
      text: asrOcrText,
      segments: asrOcrSegs,
    },
  });

  // === 阶段 7: 融合重叠段 + 最终去重 ===
  // Write asr_ocr_fused.json — fixOverlap fused result
  const maxAdvanceMs = ctx.input?.stages?.mix_audio?.maxAdvanceMs ?? 500;
  const fix = fixOverlap(
    asrOcrSegs,
    cleanFramesResult.frames,
    filteredResult.result.segments,
    maxAdvanceMs,
  ).filter((s) => s.end_ms > s.start_ms);

  // 最终去重：相邻且文本相同的段合并为一段，防止 OCR 噪声把一句台词切成多段
  // （例如"娘带着我们门爬了七座山才到"中间插入"门"字的 OCR 噪声）
  const merged: OcrSegment[] = [];
  for (const s of fix) {
    const prev = merged[merged.length - 1];
    if (prev && prev.text.trim() === s.text.trim() && s.start_ms - prev.end_ms <= 2000) {
      prev.end_ms = s.end_ms;
      if (s.text_confidence !== undefined) {
        prev.text_confidence =
          prev.text_confidence !== undefined
            ? (prev.text_confidence + s.text_confidence) / 2
            : s.text_confidence;
      }
    } else {
      merged.push({ ...s });
    }
  }

  const fixText = merged.map((s) => s.text).join(" ");
  writeJson(join(outDir, "asr_ocr_fused.json"), {
    _engine: "asr_ocr",
    _fusion_params: {
      strategy: "end2fps",
      maxAdvanceMs,
      ocrCalls: filteredResult.meta.segment_count,
      asrSegs: asrRawLen,
      asrSplits: asrSegs.length,
      fixSegs: merged.length,
    },
    result: {
      text: fixText,
      segments: merged,
    },
  });

  // === 阶段 8 (可选): LLM 文本纠错 ===
  // asr_ocr_fused_llm_fix.json：仅当 args.llmFix 开启时运行
  if (args.llmFix) {
    const llmFixedSegments = await ocrLlmFix(merged, ctx.input.task.sourceLang ?? "zh", args);
    writeJson(join(outDir, "asr_ocr_fused_llm_fix.json"), {
      result: {
        text: llmFixedSegments.map((s) => s.text).join(" "),
        segments: llmFixedSegments,
      },
    });
  }

  log(
    `filter-segment done (dropped below adjusted_confidence_threshold=${asrOcrFixArg.adjusted_confidence_threshold}) → ${asrRawLen} ASR → ${asrSegs.length} split → ${asrOcrSegs.length} merged, ${fix.length} fused`,
  );

  await setStage(taskDir, "asr_ocr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
  });
}
