import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir, writeJson, readJson } from "@repo/core/utils/fileOps";
import {
  emitLog,
  nowISO,
  probeVideoResolution,
  video_source_path,
} from "@repo/core/stages/utils/utils.ts";
import { fixOverlap, mergeFrames, toOcrFiltered } from "@repo/core/stages/ocr/ocrMerge";
import {
  computeBoxYStats,
  computeSegmentAdjustments,
  build_ocr_frames_line_adjust,
  get_ocr_frames_line_filtered,
  aggregate_boxes,
  YStats,
} from "../ocr/utils.ts";
import { newOcrEngine, type OCRRuntime } from "../../ml/subtitle_ocr/ocr.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { Segment } from "@repo/core/ml/subtitle_ocr/types";
import { LineAdjustedArgs } from "@repo/core/ml/subtitle_ocr/input";
import { t } from "@repo/shared/i18n/server";
import { buildOcrFixSystemPrompt, ocrSegmentsToPrompt } from "@repo/core/ml/llm/ocr_llm_fix";
import { chat_completions } from "@repo/core/ml/llm/openai";
import { parseLines } from "@repo/core/ml/llm/srt_shared";
import { LlmArgs, LlmFixArgs } from "@repo/core/ml/llm/input";
import { AsrResult } from "../asr/types.ts";
import { FrameResult } from "@repo/subtitle-ocr/types";

export async function stageAsrOcrFix(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  const args = ctx.input?.stages?.asr_ocr_fix;
  await setStage(taskDir, "asr_ocr_fix", {
    last_message: "Fusing ASR + OCR...",
    progress: 0,
  });

  const asrOcrFixDir = resolve(taskDir, "asr_ocr_fix");

  // Read inputs
  const asrFile = join(taskDir, "asr", "asr.json");
  const asrSplitFile = join(taskDir, "asr_ocr_pre", "asr_split.json");
  const ocrFramesFile = join(taskDir, "asr_ocr", "ocr_frames.json");

  if (!existsSync(asrFile)) throw new Error(`asr.json not found: ${asrFile}`);
  if (!existsSync(asrSplitFile)) throw new Error(`asr_split.json not found, run asr_ocr_pre first`);
  if (!existsSync(ocrFramesFile)) throw new Error(`ocr_frames.json not found, run asr_ocr first`);

  ensureDir(asrOcrFixDir, ctx);

  const asrRawLen = (await readJson<AsrResult>(asrFile, ctx)).result.segments.length;
  const asrSplitData = await readJson(asrSplitFile, ctx);
  const ocrFramesData = await readJson(ocrFramesFile, ctx);

  const asrSegs: Segment[] = (asrSplitData.result?.segments ?? []).map((s: any) => ({
    text: s.text,
    start: s.start,
    end: s.end,
  }));

  const rawFrames: FrameResult[] = ocrFramesData._frames_raw ?? [];

  const asrOcrFixCfg = ctx.input?.stages?.asr_ocr_fix;
  const textScore = asrOcrFixCfg?.textScore ?? 0.45;

  // --- Re-sample: supplement frames around high-confidence single-frame gaps ---
  const RESAMPLE_CONF_THRESH = 0.6;
  const RESAMPLE_STEP_MS = 100;
  const RESAMPLE_RANGE_MS = 500;

  const isolatedInfos: string[] = [];
  const candidateTs = new Set<number>();
  for (let i = 0; i < rawFrames.length; i++) {
    const f = rawFrames[i];
    if (!f.text || f.confidence < RESAMPLE_CONF_THRESH) continue;
    const hasNearbySameText = rawFrames.some(
      (other, j) =>
        j !== i &&
        other.text === f.text &&
        Math.abs(other.timestamp - f.timestamp) <= RESAMPLE_RANGE_MS,
    );
    if (hasNearbySameText) continue;
    const prevTs = i > 0 ? rawFrames[i - 1].timestamp : -Infinity;
    const nextTs = i < rawFrames.length - 1 ? rawFrames[i + 1].timestamp : Infinity;
    const gapBefore = f.timestamp - prevTs;
    const gapAfter = nextTs - f.timestamp;
    isolatedInfos.push(
      `  ts=${f.timestamp}ms  text="${f.text.slice(0, 30)}"  conf=${f.confidence}  gapBefore=${gapBefore}ms  gapAfter=${gapAfter}ms`,
    );
    for (
      let t = f.timestamp - RESAMPLE_RANGE_MS;
      t <= f.timestamp + RESAMPLE_RANGE_MS;
      t += RESAMPLE_STEP_MS
    ) {
      if (t >= 0) candidateTs.add(t);
    }
  }
  if (isolatedInfos.length > 0) {
    emitLog(
      taskDir,
      `[asr_ocr_fix] ${isolatedInfos.length} isolated high-confidence frames:\n${isolatedInfos.join("\n")}`,
    );
  }

  const existingTs = new Set(rawFrames.map((f) => f.timestamp));
  const newTs = [...candidateTs].filter((t) => !existingTs.has(t)).sort((a, b) => a - b);

  if (newTs.length > 0) {
    emitLog(
      taskDir,
      `[asr_ocr_fix] Re-sampling ${newTs.length} frames at ${RESAMPLE_STEP_MS}ms steps...`,
    );

    const videoPath = video_source_path(ctx);
    const resampleDir = join(asrOcrFixDir, "resampled_frames");
    ensureDir(resampleDir, ctx);

    let extracted = 0;
    for (const ts of newTs) {
      const fp = join(resampleDir, `frame_${ts.toString().padStart(7, "0")}.jpg`);
      const r = spawnSync(
        "ffmpeg",
        ["-y", "-ss", String(ts / 1000), "-i", videoPath, "-frames:v", "1", "-qscale:v", "2", fp],
        { timeout: 15_000 },
      );
      if (r.status === 0) extracted++;
    }
    emitLog(taskDir, `[asr_ocr_fix] Extracted ${extracted}/${newTs.length} resampled frames`);

    if (extracted > 0) {
      const runtime = (ocrFramesData._engine ?? "ort-cpp") as OCRRuntime;
      const device = (ocrFramesData._device ?? "cpu") as any;
      const engine = await newOcrEngine(runtime, device);

      const frameFiles = readdirSync(resampleDir)
        .filter((f) => f.endsWith(".jpg"))
        .sort();
      const ocrResults = await engine.ocrFrames(resampleDir, frameFiles, { textScore });
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
        const before = rawFrames.length;
        rawFrames.push(...newFrames);
        rawFrames.sort((a, b) => a.timestamp - b.timestamp);
        emitLog(
          taskDir,
          `[asr_ocr_fix] Added ${newFrames.length} OCR frames (${before} → ${rawFrames.length})`,
        );

        writeJson(
          join(asrOcrFixDir, "ocr_frames.json"),
          { ...ocrFramesData, _frames_raw: rawFrames },
          ctx,
        );
      }
    }
  }

  const lineAdjustedThreshold = asrOcrFixCfg?.lineAdjustedThreshold ?? 0.5;

  // Compute per-line Y statistics from raw frames
  const yStats = computeBoxYStats(rawFrames);

  // 1. ocr_frames_line_adjust.json: annotate each line with outlier info
  const annotatedFrames = build_ocr_frames_line_adjust(rawFrames, yStats, {
    lineAdjustedThreshold,
  });

  writeJson(
    join(asrOcrFixDir, "ocr_frames_line_adjust.json"),
    {
      _engine: "asr_ocr_fix",
      _line_stats: yStats,
      _frame_count: rawFrames.length,
      _config: { lineAdjustedThreshold },
      frames: annotatedFrames,
    },
    ctx,
  );

  // 2. Filter noise lines at frame level, then merge into segments
  const cleanFrames: FrameResult[] = get_ocr_frames_line_filtered(annotatedFrames);
  const cleanYStats = computeBoxYStats(cleanFrames);

  writeJson(
    join(asrOcrFixDir, "ocr_frames_line_filtered.json"),
    {
      _engine: "asr_ocr_fix",
      _line_stats: cleanYStats,
      _frame_count: cleanFrames.length,
      _frames_raw: cleanFrames,
    },
    ctx,
  );

  const { segments: ocrSegs, text: ocrText } = mergeFrames(cleanFrames, {
    mergeSubstring: args?.mergeSubstring,
  });

  if (!asrSegs.length) throw new Error("No ASR segments found");
  if (!ocrSegs.length) throw new Error("No OCR segments found (empty asr_ocr.json)");

  const { height: videoHeight } = probeVideoResolution(video_source_path(ctx));
  const isoThresholdMs = asrOcrFixCfg?.isoThresholdMs ?? 1500;
  const adjustYWeight = asrOcrFixCfg?.adjustYWeight ?? 0.8;
  const adjustIsoWeight = asrOcrFixCfg?.adjustIsoWeight ?? 0.2;
  const adjustYFactor = asrOcrFixCfg?.adjustYFactor ?? 0.08;
  const adjustedSegs = computeSegmentAdjustments(ocrSegs, cleanFrames, cleanYStats, videoHeight, {
    ...asrOcrFixCfg,
  });

  writeJson(
    join(asrOcrFixDir, "ocr_merged.json"),
    {
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
    },
    ctx,
  );

  // ocr_filtered.json：以 adjustedSegs 为输入，按 adjustedConfidence 过滤（Y 偏移 + 孤立惩罚）
  const { segments: ocrSegsMerged, dropped } = toOcrFiltered(adjustedSegs, textScore);

  writeJson(
    join(asrOcrFixDir, "ocr_filtered.json"),
    {
      audio_info: {
        duration: ocrSegsMerged.length > 0 ? ocrSegsMerged[ocrSegsMerged.length - 1].end : 0,
      },
      _boundary: "ocr",
      _fusion_params: { strategy: "end2fps", ocrCalls: ocrSegsMerged.length, textScore, dropped },
      result: {
        text: ocrSegsMerged.map((s) => s.text).join(" "),
        segments: ocrSegsMerged,
      },
    },
    ctx,
  );

  // asr_ocr_merged.json：复用 ocr_filtered.json 的 segments，时间边界对齐到 ASR
  // 对每个 OCR segment，找到时间上最重叠的 ASR segment，用 ASR 的 start/end 作为新边界
  const asrOcrSegs: Segment[] = ocrSegsMerged.map((seg) => {
    let bestAsr: Segment | undefined = undefined;
    let bestOverlap = 0;
    for (const asr of asrSegs) {
      let overlap: number;
      if (seg.start === seg.end) {
        overlap = seg.start >= asr.start && seg.start <= asr.end ? 1 : 0;
      } else {
        overlap = Math.max(0, Math.min(seg.end, asr.end) - Math.max(seg.start, asr.start));
      }
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        bestAsr = asr;
      }
    }
    return {
      text: seg.text,
      start: bestAsr ? bestAsr.start : seg.start,
      end: bestAsr ? bestAsr.end : seg.end,
      confidence: seg.confidence,
      box_y: seg.box_y,
    };
  });

  const asrOcrText = asrOcrSegs.map((s) => s.text).join(" ");

  writeJson(
    join(asrOcrFixDir, "asr_ocr_merged.json"),
    {
      audio_info: { duration: asrOcrSegs.length > 0 ? asrOcrSegs[asrOcrSegs.length - 1].end : 0 },
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
    },
    ctx,
  );

  // Write asr_ocr_fused.json — fixOverlap fused result
  const maxAdvanceMs = ctx.input?.stages?.merge_audio?.maxAdvanceMs ?? 500;
  const fix = fixOverlap(asrOcrSegs, cleanFrames, ocrSegsMerged, maxAdvanceMs).filter(
    (s) => s.end > s.start,
  );

  // 最终去重：相邻且文本相同的段合并为一段，防止 OCR 噪声把一句台词切成多段
  // （例如"娘带着我们门爬了七座山才到"中间插入"门"字的 OCR 噪声）
  const merged: Segment[] = [];
  for (const s of fix) {
    const prev = merged[merged.length - 1];
    if (prev && prev.text.trim() === s.text.trim() && s.start - prev.end <= 2000) {
      prev.end = s.end;
      if (s.confidence !== undefined) {
        prev.confidence =
          prev.confidence !== undefined ? (prev.confidence + s.confidence) / 2 : s.confidence;
      }
    } else {
      merged.push({ ...s });
    }
  }

  const fixText = merged.map((s) => s.text).join(" ");
  writeJson(
    join(asrOcrFixDir, "asr_ocr_fused.json"),
    {
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
    },
    ctx,
  );

  // asr_ocr_fused_llm_fix.json
  const ocrLlmFix = async (segments: Segment[], args: LlmArgs) => {
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
    writeJson(
      join(asrOcrFixDir, "asr_ocr_fused_llm_fix.json"),
      {
        result: {
          text: llmFixedSegments.map((s) => s.text).join(" "),
          segments: llmFixedSegments,
        },
      },
      ctx,
    );
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
