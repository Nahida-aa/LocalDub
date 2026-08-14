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
import { ensureDir, writeJson } from "@repo/util/file_op";
import { AsrResult } from "@repo/subtitle-asr/types";
import { log } from "@repo/util/log";
import { resolveLanguage } from "../05_translate/utils";

export async function stageAsrFix(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const args = ctx.input.stages.asr_fix;
  const taskDir = ctx.task.task_dir;
  const asrFixDir = join(taskDir, "asr_fix");
  const asrFile = args.asrFilePath ?? join(taskDir, "asr", "asr.json");
  const { srcLang } = resolveLanguage(ctx);
  const srtFile = join(asrFixDir, "asr_fix.json");

  if (!existsSync(asrFile)) {
    throw new Error(`ASR file not found: ${asrFile}; run ASR stage first`);
  }

  const data = await readJson<AsrResult>(asrFile);
  let segments = (data.result?.segments).filter(
    (s) => s.text && (data.meta.audio_duration ? s.start_ms < data.meta.audio_duration : true),
  );

  if (!segments.length) throw new Error("ASR result has no segments.");

  const llmFix = args.llmFix;

  // Step 1: LLM correction (before padding, to fix text)
  if (llmFix) {
    const sourceLangLabel = t(srcLang);
    const llmModel = args.llmModel;
    const llmApiBase = args.llmApiBase;
    const domainHint = args.domainHint;

    if (domainHint) emitLog(taskDir, `[ASR Fix] domainHint: ${domainHint}`);

    await setStage(taskDir, "asr_fix", {
      last_message: `LLM fixing ${segments.length} segments...`,
    });

    const prompt = segmentsToPrompt(segments);
    log(`LLM fixing ${segments.length} segs (model=${llmModel})...`);

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
      log(`LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    } else {
      log(`LLM response parse failed, keeping original text`);
    }
  }

  const resultText = segments.map((s) => s.text).join(" ");
  ensureDir(asrFixDir);
  writeJson(srtFile, {
    result: { text: resultText, segments },
    meta: {
      audio_duration: data.meta.audio_duration,
      llm_fixed: llmFix,
    },
  });

  emitLog(taskDir, `[ASR Fix] Written ${segments.length} segs to asr_fix.json`);

  await setStage(taskDir, "asr_fix", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : "Fixed",
  });
}
