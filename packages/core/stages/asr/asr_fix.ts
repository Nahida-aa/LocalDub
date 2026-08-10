import { readJson } from "@repo/core/utils/fileOps";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { readInputArgs } from "@repo/core/input/input";
import { emitLog, nowISO } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { segmentsToPrompt, buildAsrFixSystemPrompt } from "@repo/core/ml/llm/asr_llm_fix.ts";
import { chat_completions } from "@repo/core/ml/llm/openai.ts";
import { parseLines } from "@repo/core/ml/llm/srt_shared.ts";
import { t } from "@repo/shared/i18n/server";
import { srtTime } from "@repo/core/utils/utils";
import { ensureDir, writeJson } from "@repo/util/file_op";

function padSegments(segments: any[], startPad = 100, endPad = 300): any[] {
  if (!segments.length) return segments;
  const minGap = 50;

  const startPadAt = (idx: number): number => {
    const origStart = segments[idx].start;
    if (idx === 0) return Math.max(0, origStart - startPad);
    const prevEnd = segments[idx - 1].end;
    const gap = origStart - prevEnd;
    const total = startPad + endPad;
    if (gap >= total + minGap) return origStart - startPad;
    if (gap > minGap) {
      const share = ((gap - minGap) * startPad) / total;
      return origStart - share;
    }
    return prevEnd + gap / 2;
  };

  const endPadAt = (idx: number): number => {
    const origEnd = segments[idx].end;
    if (idx === segments.length - 1) {
      return origEnd + endPad;
    }
    const nextStart = segments[idx + 1].start;
    const gap = nextStart - origEnd;
    const total = startPad + endPad;
    if (gap >= total + minGap) return origEnd + endPad;
    if (gap > minGap) {
      const share = ((gap - minGap) * endPad) / total;
      return origEnd + share;
    }
    return origEnd + gap / 2;
  };

  return segments.map((s, idx) => {
    const newStart = startPadAt(idx);
    const newEnd = endPadAt(idx);
    return { ...s, start: Math.max(0, newStart), end: newEnd };
  });
}

export async function stageAsrFix(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  const asrFixDir = join(taskDir, "asr_fix");
  const asrFile = ctx.input?.stages?.asr_fix?.asrFilePath ?? join(taskDir, "asr", "asr.json");
  const srtFile = join(asrFixDir, "asr_fix.json");

  if (!existsSync(asrFile)) {
    throw new Error(`ASR file not found: ${asrFile}; run ASR stage first`);
  }

  const data = await readJson(asrFile);
  let segments: any[] = (data.result?.segments || [])
    .map((s: any) => ({ text: (s.text || "").trim(), start: s.start, end: s.end }))
    .filter(
      (s: any) => s.text && (data.audio_info?.duration ? s.start < data.audio_info.duration : true),
    );

  if (!segments.length) throw new Error("ASR result has no segments.");

  const cfg = readInputArgs().stages?.asr_fix;
  const llmFix = cfg?.llmFix ?? false;
  const segmentPad = cfg?.segmentPad ?? true;

  emitLog(taskDir, `[ASR Fix] ${segments.length} segs, llmFix=${llmFix}, segmentPad=${segmentPad}`);

  // Step 1: LLM correction (before padding, to fix text)
  if (llmFix) {
    const sourceLangLabel = t(ctx.input.task.sourceLang ?? "zh");
    const llmModel = cfg?.llmModel || "gemma4:31b-cloud";
    const llmApiBase = cfg?.llmApiBase || "http://localhost:11434/v1";
    const domainHint = cfg?.domainHint;

    if (domainHint) emitLog(taskDir, `[ASR Fix] domainHint: ${domainHint}`);

    await setStage(taskDir, "asr_fix", {
      last_message: `LLM fixing ${segments.length} segments...`,
    });

    const prompt = segmentsToPrompt(segments);
    emitLog(taskDir, `[ASR Fix] LLM fixing ${segments.length} segs (model=${llmModel})...`);

    const t0 = performance.now();
    const fixed = await chat_completions(prompt, {
      model: llmModel,
      apiBase: llmApiBase,
      systemPrompt: buildAsrFixSystemPrompt(sourceLangLabel, domainHint),
    });
    const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseLines(fixed, segments.length);
    if (fixedTexts) {
      segments = segments.map((s: any, i: number) => ({ ...s, text: fixedTexts[i] }));
      emitLog(taskDir, `[ASR Fix] LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    } else {
      emitLog(taskDir, `[ASR Fix] LLM response parse failed, keeping original text`);
    }
  }

  // Step 2: segment padding
  if (segmentPad) {
    emitLog(taskDir, `[ASR Fix] Applying segment padding...`);
    segments = padSegments(segments);
  } else {
    emitLog(taskDir, `[ASR Fix] Segment padding disabled`);
  }

  segments = segments.map((s: any) => ({
    ...s,
    start_fmt: srtTime(s.start),
    end_fmt: srtTime(s.end),
  }));
  const resultText = segments.map((s: any) => s.text).join(" ");
  ensureDir(asrFixDir);
  writeJson(srtFile, {
    audio_info: data.audio_info || {},
    result: { text: resultText, segments },
    _llm_fixed: llmFix,
  });

  emitLog(taskDir, `[ASR Fix] Written ${segments.length} segs to asr_fix.json`);

  await setStage(taskDir, "asr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : "Fixed",
  });
}
