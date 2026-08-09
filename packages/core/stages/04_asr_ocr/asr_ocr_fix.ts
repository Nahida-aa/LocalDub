import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { readJson } from "@repo/core/utils/fileOps";
import {
  emitLog,
  nowISO,
  probeVideoResolution,
  video_source_path,
} from "@repo/core/stages/utils/utils.ts";
import { fixOverlap, mergeFrames, toOcrFiltered } from "@repo/subtitle-ocr/ocr_fix/merge_frames";
import { computeBoxYStats, YStats } from "@repo/subtitle-ocr/ocr_fix/stats";
import { computeSegmentAdjustments } from "../ocr/utils.ts";
import {
  build_ocr_frames_box_adjust,
  get_ocr_frames_box_filtered,
} from "@repo/subtitle-ocr/ocr_fix/box_adjusted";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { t } from "@repo/shared/i18n/server";
import { buildOcrFixSystemPrompt, ocrSegmentsToPrompt } from "@repo/core/ml/llm/ocr_llm_fix";
import { chat_completions } from "@repo/core/ml/llm/openai";
import { parseLines } from "@repo/core/ml/llm/srt_shared";
import { LlmArgs, LlmFixArgs } from "@repo/llm/llm_fix_args";
import { AsrResult } from "../asr/types.ts";
import { FrameResult, OcrFramesResult, OCRRuntime, OcrSegment } from "@repo/subtitle-ocr/types";
import { writeJson, ensureDir } from "@repo/util/file_op";
import {
  collect_resample_candidate_ms,
  hasNearbySameText,
  resample_candidate_to_ocr_frames,
  resample_to_ocr_frames,
} from "@repo/subtitle-ocr/ocr_fix/resample";

import { extract_frames } from "@repo/subtitle-ocr/ffmpeg_util";
import { to } from "@repo/shared/lib/utils/try";
import { log } from "@repo/util/log";

export async function stageAsrOcrFix(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  const args = ctx.input?.stages?.asr_ocr_fix;
  await setStage(taskDir, "asr_ocr_fix", {
    last_message: "Fusing ASR + OCR...",
    progress: 0,
  });

  const asrOcrFixDir = resolve(taskDir, "asr_ocr_fix");

  // === 读取上游产物 ===
  // asr.json          : ASR 原始分段（whisper 等）
  // asr_split.json    : asr_ocr_pre 按标点/词切分后的 ASR 段（驱动抽帧时间戳）
  // ocr_frames.json   : asr_ocr 阶段产出的逐帧 OCR 原始结果（OcrFramesResult 格式）
  const asrFile = join(taskDir, "asr", "asr.json");
  const asrSplitFile = join(taskDir, "asr_ocr_pre", "asr_split.json");
  const ocrFramesFile = join(taskDir, "asr_ocr", "asr_ocr_frames.json");

  if (!existsSync(asrFile)) throw new Error(`asr.json not found: ${asrFile}`);
  if (!existsSync(asrSplitFile)) throw new Error(`asr_split.json not found, run asr_ocr_pre first`);
  if (!existsSync(ocrFramesFile)) throw new Error(`ocr_frames.json not found, run asr_ocr first`);

  ensureDir(asrOcrFixDir);

  const asrRawLen = (await readJson<AsrResult>(asrFile, ctx)).result.segments.length;
  const asrSplitData = await readJson(asrSplitFile, ctx);
  const ocrFramesData = await readJson<OcrFramesResult>(ocrFramesFile, ctx);

  // ASR 切分后的段（只取 text/start/end，用于后续时间边界对齐）
  const asrSegs: OcrSegment[] = (asrSplitData.result?.segments ?? []).map((s: any) => ({
    text: s.text,
    start: s.start,
    end: s.end,
  }));

  const asrOcrFixArg = ctx.input.stages.asr_ocr_fix;
  const textScore = asrOcrFixArg?.text_score_threshold ?? 0.45;

  const videoPath = video_source_path(ctx);

  const frames: FrameResult[] = asrOcrFixArg.is_resample
    ? await resample_to_ocr_frames(ocrFramesData, videoPath, asrOcrFixDir, asrOcrFixArg) // === 阶段 1: 重采样补充帧 ===
    : ocrFramesData.frames; // 逐帧 OCR 原始结果；后续的重采样/过滤都作用在 rawFrames 上

  // === 阶段 2: 逐行 Y 统计 + 离群标注 + 过滤 ===
  // 从（含重采样帧的）rawFrames 算整体 Y 统计（典型行位置/高度，含 avg/mode/median/各种 height）
  const yStats = computeBoxYStats(frames);

  // 2.1 ocr_frames_line_adjust.json: 给每个 box 标注离群信息（is_outlier / adjustedConfidence / 上下偏移比）
  //     基于整帧的 yStats 判断某行是否偏离典型字幕行位置/高度
  const annotatedFrames = build_ocr_frames_box_adjust(frames, yStats, asrOcrFixArg);

  writeJson(join(asrOcrFixDir, "ocr_frames_box_adjust.json"), annotatedFrames);

  // 2.2 ocr_frames_line_filtered.json: 过滤被标 is_outlier 的 box（box 级），整帧清空则丢帧（帧级）；
  //     部分 box 离群则按剩余干净 box 重建该帧
  const cleanFramesResult = get_ocr_frames_box_filtered(annotatedFrames.frames);

  writeJson(join(asrOcrFixDir, "ocr_frames_box_filtered.json"), cleanFramesResult);

  // === 阶段 3: 合并 OCR 逐帧为字幕段 ===
  const { segments: ocrSegs, text: ocrText } = mergeFrames(cleanFramesResult.frames, {
    mergeSubstring: args.mergeSubstring,
    dedup_edit_distance: args.dedup_edit_distance,
  });
  writeJson(join(asrOcrFixDir, "ocr_merged.json"), {
    result: {
      text: ocrText,
      segments: ocrSegs,
    },
  });

  if (!asrSegs.length) throw new Error("No ASR segments found");
  if (!ocrSegs.length) throw new Error("No OCR segments found (empty asr_ocr.json)");

  // === 阶段 4: OCR 段时间边界调整（对齐到帧 Y 统计 + 孤立惩罚）===
  const { height: videoHeight } = probeVideoResolution(video_source_path(ctx));
  const isoThresholdMs = asrOcrFixArg?.isoThresholdMs ?? 1500;
  const adjustYWeight = asrOcrFixArg?.adjustYWeight ?? 0.8;
  const adjustIsoWeight = asrOcrFixArg?.adjustIsoWeight ?? 0.2;
  const adjustYFactor = asrOcrFixArg?.adjustYFactor ?? 0.08;
  const adjustedSegs = computeSegmentAdjustments(
    ocrSegs,
    cleanFramesResult.frames,
    cleanFramesResult.meta.y_stats,
    videoHeight,
    {
      ...asrOcrFixArg,
    },
  );

  writeJson(join(asrOcrFixDir, "ocr_merged_adjust.json"), {
    _engine: "asr_ocr",
    _fusion_params: {
      strategy: "end2fps",
      isoThresholdMs,
      adjustYWeight,
      adjustIsoWeight,
      adjustYFactor,
    },
    result: {
      text: adjustedSegs.map((s) => s.text).join(" "),
      segments: adjustedSegs,
    },
  });

  // === 阶段 5: 按 textScore 过滤低置信段 ===
  // ocr_filtered.json：以 adjustedSegs 为输入，按 adjustedConfidence 过滤（Y 偏移 + 孤立惩罚）
  const { segments: ocrSegsMerged, dropped } = toOcrFiltered(adjustedSegs, textScore);

  writeJson(join(asrOcrFixDir, "ocr_filtered.json"), {
    audio_info: {
      duration: ocrSegsMerged.length > 0 ? ocrSegsMerged[ocrSegsMerged.length - 1].end_ms : 0,
    },
    _boundary: "ocr",
    _fusion_params: { strategy: "end2fps", ocrCalls: ocrSegsMerged.length, textScore, dropped },
    result: {
      text: ocrSegsMerged.map((s) => s.text).join(" "),
      segments: ocrSegsMerged,
    },
  });

  // === 阶段 6: 时间边界对齐到 ASR ===
  // asr_ocr_merged.json：复用 ocr_filtered.json 的 segments，时间边界对齐到 ASR
  // 对每个 OCR segment，找到时间上最重叠的 ASR segment，用 ASR 的 start/end 作为新边界
  const asrOcrSegs: OcrSegment[] = ocrSegsMerged.map((seg) => {
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
      confidence: seg.text_confidence,
      y_range: seg.y_range,
    };
  });

  const asrOcrText = asrOcrSegs.map((s) => s.text).join(" ");

  writeJson(join(asrOcrFixDir, "asr_ocr_merged.json"), {
    audio_info: { duration: asrOcrSegs.length > 0 ? asrOcrSegs[asrOcrSegs.length - 1].end_ms : 0 },
    _engine: "asr_ocr",
    _fusion_params: {
      strategy: "end2fps",
      ocrCalls: ocrSegsMerged.length,
      asrSegs: asrRawLen,
      asrSplits: asrSegs.length,
      textScore,
      dropped,
    },
    result: {
      text: asrOcrText,
      segments: asrOcrSegs,
    },
  });

  // === 阶段 7: 融合重叠段 + 最终去重 ===
  // Write asr_ocr_fused.json — fixOverlap fused result
  const maxAdvanceMs = ctx.input?.stages?.merge_audio?.maxAdvanceMs ?? 500;
  const fix = fixOverlap(asrOcrSegs, cleanFramesResult.frames, ocrSegsMerged, maxAdvanceMs).filter(
    (s) => s.end_ms > s.start_ms,
  );

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
  writeJson(join(asrOcrFixDir, "asr_ocr_fused.json"), {
    _engine: "asr_ocr",
    _fusion_params: {
      strategy: "end2fps",
      maxAdvanceMs,
      ocrCalls: ocrSegsMerged.length,
      asrSegs: asrRawLen,
      asrSplits: asrSegs.length,
      fixSegs: merged.length,
      textScore,
      dropped,
    },
    result: {
      text: fixText,
      segments: merged,
    },
  });

  // === 阶段 8 (可选): LLM 文本纠错 ===
  // asr_ocr_fused_llm_fix.json：仅当 args.llmFix 开启时运行
  const ocrLlmFix = async (segments: OcrSegment[], args: LlmArgs) => {
    const sourceLangLabel = t(ctx.input.task.sourceLang ?? "zh");
    const llmModel = args.llmModel;
    const llmApiBase = args.llmApiBase;
    const domainHint = args.domainHint;
    if (domainHint) emitLog(taskDir, `[asr_ocr_fix] domainHint: ${domainHint}`);
    const prompt = ocrSegmentsToPrompt(segments);
    emitLog(taskDir, `[asr_ocr_fix] LLM fixing ${segments.length} segs (model=${llmModel})...`);

    const t0 = performance.now();
    const fixed = await chat_completions(prompt, {
      model: llmModel,
      apiBase: llmApiBase,
      systemPrompt: buildOcrFixSystemPrompt(sourceLangLabel, domainHint),
    });
    const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseLines(fixed, segments.length);
    if (fixedTexts) {
      emitLog(taskDir, `[asr_ocr_fix] LLM fixed ${segments.length} segs in ${elapsedSec}s`);
      return segments.map((s, i) => ({ ...s, text: fixedTexts[i] }));
    } else {
      emitLog(taskDir, `[asr_ocr_fix] LLM response parse failed, keeping original text`);
      throw new Error("LLM response parse failed");
    }
  };
  if (args?.llmFix) {
    const llmFixedSegments = await ocrLlmFix(merged, args);
    writeJson(join(asrOcrFixDir, "asr_ocr_fused_llm_fix.json"), {
      result: {
        text: llmFixedSegments.map((s) => s.text).join(" "),
        segments: llmFixedSegments,
      },
    });
  }

  emitLog(
    taskDir,
    `[asr_ocr_fix] ${ocrSegs.length} OCR segs (dropped ${dropped} below textScore=${textScore}) → ${asrRawLen} ASR → ${asrSegs.length} split → ${asrOcrSegs.length} merged, ${fix.length} fused`,
  );

  await setStage(taskDir, "asr_ocr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
  });
}
