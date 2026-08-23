import { $, spawn } from "bun";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir, writeJson } from "@repo/util/file_op";
import { nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { chat_completions } from "@repo/core/ml/llm/openai.ts";
import { ocrSegmentsToPrompt, buildOcrFixSystemPrompt } from "@repo/core/ml/llm/ocr_llm_fix.ts";
import { parseLines } from "@repo/core/ml/llm/srt_shared.ts";
import { t } from "@repo/shared/i18n/server";
import { srtTime } from "@repo/core/utils/utils";
import { REPO_ROOT } from "@repo/config/root";
import { log } from "@repo/util/log";
import type { OcrSegmentWithAdjust } from "@repo/subtitle-ocr/types";
import { OcrSegmentFilterResult } from "@repo/subtitle-ocr/ocr_fix/segment_filter";
import { readJson } from "../../utils/fileOps";
import { ocrLlmFix } from "./llm_fix";
import { cellOcrPost, ensureOcrPostBin } from "./util";

// sf_ocr_fix：消费 sf_ocr 产出的 frames.json（OcrFramesResult），调用 subtitle-ocr-cli 的
// ocr-post 统合管线（adjust-box → filter-box → merge → adjust-segment → filter-segment）
// 一条命令串起帧合并与段时间调整，再叠加可选 LLM 修正（最后一层修复），
// 最后把结果与 LLM 修正后的段写出 `<taskDir>/sf_ocr_fix/sf_ocr_fix.json`。
//
// ocr-post 中间产物（frames_box_adjust / frames_box_filter / frames_merged /
// segment_adjust / segment_filter.json）均落盘到 sf_ocr_fix 目录，segment_filter.json
// 即最终（过滤后）字幕段。
export async function stageSfOcrFix(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  const framesFile = join(taskDir, "sf_ocr", "frames.json");
  const outDir = resolve(taskDir, "sf_ocr_fix");
  const videoFile = video_source_path(ctx);

  if (!existsSync(framesFile)) {
    throw new Error(`frames.json not found: ${framesFile}; run sf_ocr first`);
  }

  const args = ctx.input?.stages?.sf_ocr_fix;
  const llmFix = args?.llmFix;

  const filtered = await cellOcrPost(framesFile, videoFile, outDir, args);
  const adjustedSegs = filtered.result.segments ?? [];
  log(`ocr-post → ${adjustedSegs.length} segments (filtered)`);

  // === 阶段 3: LLM 修正（最后一层修复）===
  const segments: OcrSegmentWithAdjust[] = adjustedSegs;
  const llmFixFile = join(outDir, "segment_filter_llm_fix.json");

  if (args.llmFix) {
    const llmFixedSegments = await ocrLlmFix(segments, ctx.input.task.sourceLang ?? "zh", args);
    writeJson(llmFixFile, {
      result: {
        text: llmFixedSegments.map((s) => s.text).join(" "),
        segments: llmFixedSegments,
      },
    });
  }

  log(`Written ${segments.length} segs to segment_filter_llm_fix.json`);

  await setStage(taskDir, "sf_ocr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : `Merged ${segments.length} segs`,
  });
}
