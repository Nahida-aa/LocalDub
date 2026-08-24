import { LlmArgs } from "@repo/llm/llm_fix_args";
import { OcrSegment } from "@repo/subtitle-ocr/types";
import { TargetLang } from "../../../const/lang";
import { t } from "@repo/shared/i18n/server";
import { log } from "@repo/util/log";
import { buildOcrFixSystemPrompt, ocrSegmentsToPrompt } from "./ocr_prompt";
import { chat_completions } from "@repo/llm/openai";
import { parseLines } from "../../utils/srt_llm";

export const ocrLlmFix = async (segments: OcrSegment[], sourceLang: TargetLang, args: LlmArgs) => {
  const sourceLangLabel = t(sourceLang);
  const llmModel = args.llmModel;
  const llmApiBase = args.llmApiBase;
  const domainHint = args.domainHint;
  if (domainHint) log(`domainHint: ${domainHint}`);
  const prompt = ocrSegmentsToPrompt(segments);
  log(`LLM fixing ${segments.length} segs (model=${llmModel})...`);

  const t0 = performance.now();
  const fixed = await chat_completions(prompt, {
    model: llmModel,
    apiBase: llmApiBase,
    systemPrompt: buildOcrFixSystemPrompt(sourceLangLabel, domainHint),
  });
  const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

  const fixedTexts = parseLines(fixed, segments.length);
  if (fixedTexts) {
    log(`LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    return segments.map((s, i) => ({ ...s, text: fixedTexts[i] }));
  } else {
    log(`LLM response parse failed, keeping original text`);
    throw new Error("LLM response parse failed");
  }
};
