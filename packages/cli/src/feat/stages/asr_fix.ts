import { readJson, writeJson } from './fileOps.ts';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { readConfig } from '../config/config.ts';
import { emitLog, nowISO, srtTime, updateStageDB } from './utils/utils.ts';
import { segmentsToPrompt, parseLines, fixWithLLM } from './asr/llm.ts';

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
      const share = (gap - minGap) * startPad / total;
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
      const share = (gap - minGap) * endPad / total;
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

export async function stageAsrFix(taskId: string, sessionPath: string) {
  const metadataDir = join(sessionPath, 'metadata');
  const asrFile = join(metadataDir, 'asr.json');
  const srtFile = join(metadataDir, 'asr_fix.json');

  if (!existsSync(asrFile)) {
    throw new Error(`ASR file not found: ${asrFile}; run ASR stage first`);
  }

  const data = readJson(asrFile, 'ASR Fix');
  let segments: any[] = (data.result?.segments || [])
    .map((s: any) => ({ text: (s.text || '').trim(), start: s.start, end: s.end }))
    .filter((s: any) => s.text && (data.audio_info?.duration ? s.start < data.audio_info.duration : true));

  if (!segments.length) throw new Error('ASR result has no segments.');

  const cfg = readConfig().stages?.asr_fix;
  const llmFix = cfg?.llmFix ?? false;
  const segmentPad = cfg?.segmentPad ?? true;

  emitLog(taskId, `[ASR Fix] ${segments.length} segs, llmFix=${llmFix}, segmentPad=${segmentPad}`);

  // Step 1: LLM correction (before padding, to fix text)
  if (llmFix) {
    const llmModel = cfg?.llmModel || 'gemma4:31b-cloud';
    const llmApiBase = cfg?.llmApiBase || 'http://localhost:11434/v1';
    const domainHint = cfg?.domainHint;

    if (domainHint) emitLog(taskId, `[ASR Fix] domainHint: ${domainHint}`);

    await updateStageDB(taskId, 'asr_fix', {
      last_message: `LLM fixing ${segments.length} segments...`,
    });

    const prompt = segmentsToPrompt(segments);
    emitLog(taskId, `[ASR Fix] LLM fixing ${segments.length} segs (model=${llmModel})...`);

    const t0 = performance.now();
    const fixed = await fixWithLLM(prompt, { model: llmModel, apiBase: llmApiBase, domainHint });
    const elapsedSec = ((performance.now() - t0) / 1000).toFixed(1);

    const fixedTexts = parseLines(fixed, segments.length);
    if (fixedTexts) {
      segments = segments.map((s: any, i: number) => ({ ...s, text: fixedTexts[i] }));
      emitLog(taskId, `[ASR Fix] LLM fixed ${segments.length} segs in ${elapsedSec}s`);
    } else {
      emitLog(taskId, `[ASR Fix] LLM response parse failed, keeping original text`);
    }
  }

  // Step 2: segment padding
  if (segmentPad) {
    emitLog(taskId, `[ASR Fix] Applying segment padding...`);
    segments = padSegments(segments);
  } else {
    emitLog(taskId, `[ASR Fix] Segment padding disabled`);
  }

  segments = segments.map((s: any) => ({ ...s, start_fmt: srtTime(s.start), end_fmt: srtTime(s.end) }));
  const resultText = segments.map((s: any) => s.text).join(' ');
  writeJson(srtFile, {
    audio_info: data.audio_info || {},
    result: { text: resultText, segments },
    _llm_fixed: llmFix,
  }, 'ASR Fix');

  emitLog(taskId, `[ASR Fix] Written ${segments.length} segs to asr_fix.json`);

  await updateStageDB(taskId, 'asr_fix', {
    status: 'succeeded', completed_at: nowISO(), progress: 100,
    last_message: llmFix ? `LLM fixed ${segments.length} segs` : 'Fixed',
  });
}
