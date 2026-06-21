import { readJson, writeJson, ensureDir, removeFile } from './utils/fileOps.ts';
import { existsSync, readdirSync, statSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { translationFilePath, ffmpeg, nowISO, emitLog, readTaskLanguages, subtitleFilePath } from './utils/utils.ts';
import { env } from '@repo/config';
import { Context, setStage } from '../context/context.ts';

function probeDuration(file: string): number {
  const r = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', file], { stdio: ['pipe', 'pipe', 'pipe'] });
  return Math.floor(parseFloat(r.stdout.toString().trim()) * 1000) || 0;
}

function detectSpeechStartMs(wavPath: string): number {
  const durProbe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', wavPath], { stdio: ['pipe', 'pipe', 'pipe'] });
  const origDur = parseFloat(durProbe.stdout.toString().trim()) || 0;
  if (origDur <= 0) return 0;

  const tmpPath = wavPath.replace('.wav', '.trim.wav');
  const r = spawnSync(env.FFMPEG_PATH, [
    '-i', wavPath,
    '-af', 'silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1',
    '-y', tmpPath,
  ], { stdio: ['pipe', 'pipe', 'pipe'], timeout: 30_000 });

  let removedMs = 0;
  if (r.status === 0) {
    const trimProbe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', tmpPath], { stdio: ['pipe', 'pipe', 'pipe'] });
    const trimmedDur = parseFloat(trimProbe.stdout.toString().trim()) || 0;
    removedMs = Math.round((origDur - trimmedDur) * 1000);
  }
  rmSync(tmpPath, { force: true });
  return Math.max(0, removedMs);
}

function detectSpeechStartMsSeek(source: string, startMs: number, endMs: number, workDir: string): number {
  const durMs = endMs - startMs;
  if (durMs <= 0) return 0;

  const tmpPath = join(workDir, '.vad_trim.wav');
  const r = spawnSync(env.FFMPEG_PATH, [
    '-ss', String(startMs / 1000),
    '-to', String(endMs / 1000),
    '-i', source,
    '-vn',
    '-af', 'silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1',
    '-y', tmpPath,
  ], { stdio: ['pipe', 'pipe', 'pipe'], timeout: 30_000 });

  let removedMs = 0;
  if (r.status === 0) {
    const trimProbe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', tmpPath], { stdio: ['pipe', 'pipe', 'pipe'] });
    const trimmedDur = parseFloat(trimProbe.stdout.toString().trim()) || 0;
    removedMs = Math.round((durMs / 1000 - trimmedDur) * 1000);
  }
  rmSync(tmpPath, { force: true });
  return Math.max(0, removedMs);
}

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

export interface StageSplitAudioInput {
  ctx: Context;
}
export async function stageSplitAudio({
  ctx,
  
}: StageSplitAudioInput) {
  const taskId = ctx.task.id;
  const sessionPath = ctx.task.session_path
  const srtFilePath = subtitleFilePath(sessionPath, ctx.input?.subtitleSource);
  const sourceFilePath = ctx.input?.stages?.split_audio?.sourceFilePath ?? join(sessionPath, 'media', 'video_source.mp4');
  const { asrLanguage: srcLangCode, targetLanguage: dstLangCode } = readTaskLanguages(ctx);
  const metadataDir = join(sessionPath, 'metadata');
	const translationFile = translationFilePath(sessionPath, dstLangCode);
	const timingsFile = join(metadataDir, 'timings.json');
	const segmentsDir = join(sessionPath, 'segments', 'vocals');

	if (!existsSync(srtFilePath)) throw new Error(`subtitle file not found: ${srtFilePath}`);
  const vocalsFilePath = ctx.input?.stages?.split_audio?.vocalsFilePath ?? join(sessionPath, 'media', 'target_3_vocals.wav')
	const hasVocals = vocalsFilePath ? existsSync(vocalsFilePath) : false
	const sourceAudio = hasVocals ? vocalsFilePath! : sourceFilePath;

	// Read authoritative timings from srt.json (seconds)
	const srtData = await readJson(srtFilePath, ctx);
	const segmentsSrc: { text: string; start: number; end: number }[] = srtData.result?.segments;
	if (!segmentsSrc?.length) throw new Error(`${srtFilePath} has no segments`);

	// Read translated text from translation.json, or original from srt.json
	const translateEnabled = ctx.input?.stages?.translate?.enabled ?? true;
	let timings: {
		seg_idx: number;
		src: string;
		dst: string;
		src_lang: string;
		dst_lang: string;
		start_time: number;
		end_time: number;
		speaker: string;
	}[];
	if (translateEnabled) {
		if (!existsSync(translationFile))
			throw new Error(`translation file not found: ${translationFile}`);
		const transData = await readJson(translationFile, ctx);
		const translation = transData.translation;
		if (!translation?.length) throw new Error('translation.json has no segments');
		if (segmentsSrc.length !== translation.length) {
			throw new Error(`Segment count mismatch: srt (${segmentsSrc.length}) !== translation (${translation.length})`);
		}
		timings = segmentsSrc.map((seg, i) => ({
			seg_idx: i + 1,
			src: translation[i].src,
			dst: translation[i].dst,
			src_lang: translation[i].src_lang,
			dst_lang: translation[i].dst_lang,
			start_time: Math.floor(seg.start),
			end_time: Math.ceil(seg.end),
			speaker: translation[i].speaker ?? '1',
		}));
	} else {
		timings = segmentsSrc.map((seg, i) => ({
			seg_idx: i + 1,
			src: seg.text,
			dst: seg.text,
			src_lang: srcLangCode,
			dst_lang: srcLangCode,
			start_time: Math.floor(seg.start),
			end_time: Math.ceil(seg.end),
			speaker: '1',
		}));
	}

	timings = padSegments(timings);

  // Total audio duration
  let totalMs = srtData.audio_info?.duration ?? 0;
  if (!totalMs) totalMs = probeDuration(sourceAudio);

  ensureDir(segmentsDir, ctx);

  // ---- Segment cutting (dub only) ----
  if (hasVocals) {
    const anySeg = readdirSync(segmentsDir).find(f => f.endsWith('.wav'));
    if (anySeg && existsSync(translationFile) && statSync(translationFile).mtimeMs > statSync(join(segmentsDir, anySeg)).mtimeMs) {
      for (const f of readdirSync(segmentsDir)) rmSync(join(segmentsDir, f));
    }

    for (let i = 0; i < timings.length; i++) {
      const idx = String(i + 1).padStart(4, '0');
      const outPath = join(segmentsDir, `${idx}.wav`);
      if (existsSync(outPath)) continue;

      const startMs = timings[i].start_time;
      const endMs = timings[i].end_time;
      if (startMs >= endMs) {
        writeFileSync(outPath, Buffer.alloc(44));
        emitLog(sessionPath, `[split_audio] #${i + 1} invalid (${startMs} >= ${endMs}), empty wav`);
        continue;
      }

      const start = Math.max(0, startMs - 80);
      const end = Math.min(totalMs, endMs + 160);
      if (end <= start) {
        writeFileSync(outPath, Buffer.alloc(44));
        continue;
      }

      ffmpeg(['-i', sourceAudio, '-ss', String(start / 1000), '-to', String(end / 1000), '-c', 'copy', outPath]);
    }
  }

  // ---- VAD alignment ----
  const splitCfg = ctx.input?.stages?.split_audio;
  if (splitCfg?.vadAlign) {
    let corrected = false;
    for (let i = 0; i < timings.length; i++) {
      const startMs = timings[i].start_time;
      const endMs = timings[i].end_time;
      if (startMs >= endMs) continue;

      // Detect leading non-speech content (breath/silence) in ms
      const wavPath = join(segmentsDir, `${String(i + 1).padStart(4, '0')}.wav`);
      const removedMs = existsSync(wavPath)
        ? detectSpeechStartMs(wavPath)
        : detectSpeechStartMsSeek(sourceAudio, Math.max(0, startMs - 80), Math.min(totalMs, endMs + 160), segmentsDir);
      if (removedMs <= 500) continue;

      const newStartMs = startMs + removedMs - 80;
      if (newStartMs >= endMs) {
        emitLog(sessionPath, `vadAlign #${i + 1}: would exceed end (${newStartMs} >= ${endMs}), truncating`);
        continue;
      }

      emitLog(sessionPath, `vadAlign #${i + 1}: start ${startMs} → ${newStartMs} (removed ${removedMs}ms)`);

      // Re-cut WAV with corrected timing (dub only)
      if (hasVocals) {
        const newEnd = Math.min(totalMs, endMs + 160);
        if (newEnd > newStartMs) {
          ffmpeg(['-i', sourceAudio, '-ss', String(newStartMs / 1000), '-to', String(newEnd / 1000), '-c', 'copy', wavPath]);
        }
      }

      // Update timings in memory (will be written to timings.json below)
      timings[i].start_time = newStartMs;
      corrected = true;
    }

    if (corrected) {
      writeJson(timingsFile, { translation: timings }, ctx);
    }
  }

	// Write timings.json (always refresh to pick up updated OCR/OCR-fix timestamps)
	writeJson(timingsFile, { translation: timings }, ctx);

  setStage(sessionPath, 'split_audio', { status: 'succeeded', completed_at: nowISO(), progress: 100, last_message: 'Split' });
}
