import { readJson, writeJson } from '../fileOps.ts';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { readConfig } from '../../config/config.ts';
import { emitLog, nowISO, srtTime, updateStageDB } from '../utils/utils.ts';
import { buildSystemPrompt, segmentsToPrompt, fixWithLLM, parseLines } from './llm.ts';

export async function stageOcrFix(taskId: string, sessionPath: string) {
  const metadataDir = join(sessionPath, 'metadata');
  const ocrFile = join(metadataDir, 'ocr.json');
  const fixFile = join(metadataDir, 'ocr_fix.json');

  if (!existsSync(ocrFile)) {
    throw new Error(`OCR file not found: ${ocrFile}; run OCR stage first`);
  }

  const data = readJson(ocrFile, 'OCR Fix');
  let segments: any[] = (data.result?.segments || [])
    .map((s: any) => ({ text: (s.text || '').trim(), start: s.start, end: s.end }))
    .filter((s: any) => s.text);

  if (!segments.length) throw new Error('OCR result has no segments.');

  const cfg = readConfig().stages?.ocr_fix;
  const llmFix = cfg?.llmFix ?? false;

  emitLog(taskId, `[OCR Fix] ${segments.length} segs, llmFix=${llmFix}`);

  // LLM correction
  if (llmFix) {
    const llmModel = cfg?.llmModel || 'gemma4:31b-cloud';
    const llmApiBase = cfg?.llmApiBase || 'http://localhost:11434/v1';
    const domainHint = cfg?.domainHint;

    if (domainHint) emitLog(taskId, `[OCR Fix] domainHint: ${domainHint}`);

    await updateStageDB(taskId, 'ocr_fix', {
      last_message: `LLM fixing ${segments.length} segments...`,
    });

    const prompt = segmentsToPrompt(segments);
    emitLog(taskId, `[OCR Fix] LLM fixing ${segments.length} segs (model=${llmModel})...`);

    const t0 = performance.now();
    const fixed = await fixWithLLM(prompt, { model: llmModel, apiBase: llmApiBase, domainHint });
    const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseLines(fixed, segments.length);
    if (fixedTexts) {
      segments = segments.map((s: any, i: number) => ({ ...s, text: fixedTexts[i] }));
      emitLog(taskId, `[OCR Fix] LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    } else {
      emitLog(taskId, `[OCR Fix] LLM response parse failed, keeping original text`);
    }
  }

  segments = segments.map((s: any) => ({ ...s, start_fmt: srtTime(s.start), end_fmt: srtTime(s.end) }));
  const resultText = segments.map((s: any) => s.text).join(' ');
  writeJson(fixFile, {
    audio_info: data.audio_info || {},
    result: { text: resultText, segments },
    _llm_fixed: llmFix,
  }, 'OCR Fix');

  emitLog(taskId, `[OCR Fix] Written ${segments.length} segs to ocr_fix.json`);

  await updateStageDB(taskId, 'ocr_fix', {
    status: 'succeeded', completed_at: nowISO(), progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : 'Fixed',
  });
}
