import { readJson } from "@repo/core/utils/fileOps";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { emitLog, nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { chat_completions } from "@repo/core/ml/llm/openai.ts";
import { ocrSegmentsToPrompt, buildOcrFixSystemPrompt } from "@repo/core/ml/llm/ocr_llm_fix.ts";
import { parseLines } from "@repo/core/ml/llm/srt_shared.ts";
import { t } from "@repo/shared/i18n/server";
import { srtTime } from "@repo/core/utils/utils";
import { ensureDir, writeJson } from "@repo/util/file_op";

export async function stageOcrFix(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  const ocrFixDir = join(taskDir, "ocr_fix");
  const ocrFile = join(taskDir, "ocr", "ocr.json");
  const fixFile = join(ocrFixDir, "ocr_fix.json");

  ensureDir(ocrFixDir);

  if (!existsSync(ocrFile)) {
    throw new Error(`OCR file not found: ${ocrFile}; run OCR stage first`);
  }

  const data = await readJson(ocrFile, ctx);
  let segments: any[] = (data.result?.segments || [])
    .map((s: any) => ({ text: (s.text || "").trim(), start: s.start, end: s.end }))
    .filter((s: any) => s.text);

  if (!segments.length) throw new Error("OCR result has no segments.");

  const args = ctx.input.stages.ocr_fix;
  const llmFix = args?.llmFix;

  emitLog(taskDir, `[ocr_fix] ${segments.length} segs, llmFix=${llmFix}`);

  // LLM correction
  if (llmFix) {
    const sourceLangLabel = t(ctx.input.task.sourceLang ?? "zh");
    const llmModel = args.llmModel;
    const llmApiBase = args.llmApiBase;
    const domainHint = args.domainHint;

    if (domainHint) emitLog(taskDir, `[ocr_fix] domainHint: ${domainHint}`);

    await setStage(taskDir, "ocr_fix", {
      last_message: `LLM fixing ${segments.length} segments...`,
    });

    const prompt = ocrSegmentsToPrompt(segments);
    emitLog(taskDir, `[ocr_fix] LLM fixing ${segments.length} segs (model=${llmModel})...`);

    const t0 = performance.now();
    const fixed = await chat_completions(prompt, {
      model: llmModel,
      apiBase: llmApiBase,
      systemPrompt: buildOcrFixSystemPrompt(sourceLangLabel, domainHint),
    });
    const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseLines(fixed, segments.length);
    if (fixedTexts) {
      segments = segments.map((s: any, i: number) => ({ ...s, text: fixedTexts[i] }));
      emitLog(taskDir, `[ocr_fix] LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    } else {
      emitLog(taskDir, `[ocr_fix] LLM response parse failed, keeping original text`);
    }
  }

  segments = segments.map((s: any) => ({
    ...s,
    start_fmt: srtTime(s.start),
    end_fmt: srtTime(s.end),
  }));
  const resultText = segments.map((s: any) => s.text).join(" ");
  writeJson(fixFile, {
    audio_info: data.audio_info || {},
    result: { text: resultText, segments },
    _llm_fixed: llmFix,
  });

  emitLog(taskDir, `[ocr_fix] Written ${segments.length} segs to ocr_fix.json`);

  await setStage(taskDir, "ocr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : "Fixed",
  });
}
