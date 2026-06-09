import { existsSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { nowISO, updateStageDB } from './utils.ts';

function fixAsrSegments(segments: any[], startPad = 0.1, endPad = 0.3): any[] {
  if (!segments.length) return segments;
  const minGap = 0.05;

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
  const fixedFile = join(metadataDir, 'asr_fixed.json');

  if (existsSync(fixedFile) && existsSync(asrFile) && statSync(asrFile).mtimeMs <= statSync(fixedFile).mtimeMs) {
    await updateStageDB(taskId, 'asr_fix', { status: 'succeeded', completed_at: nowISO(), progress: 100, last_message: 'Already fixed' });
    return;
  }

  const data = JSON.parse(readFileSync(asrFile, 'utf-8'));
  const segments = data.result.segments;
  const durationS = (data.audio_info?.duration ?? 0) / 1000;

  const cleaned = segments
    .map((s: any) => ({ text: (s.text || '').trim(), start: s.start, end: s.end }))
    .filter((s: any) => s.text && s.start < durationS);

  if (!cleaned.length) throw new Error('ASR result has no segments.');

  const padded = fixAsrSegments(cleaned);
  writeFileSync(fixedFile, JSON.stringify({
    audio_info: data.audio_info || {},
    result: { text: data.result.text || '', segments: padded },
  }, null, 2));

  await updateStageDB(taskId, 'asr_fix', { status: 'succeeded', completed_at: nowISO(), progress: 100, last_message: 'Fixed' });
}
