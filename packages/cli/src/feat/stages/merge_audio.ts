import { readJson, writeJson, writeFile, ensureDir } from './fileOps.ts';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { readTaskLanguages, translationFilePath, ffmpeg, nowISO, updateStageDB } from './utils/utils.ts';

export async function stageMergeAudio(taskId: string, sessionPath: string) {
  const { targetLanguage: dstLangCode } = readTaskLanguages(sessionPath);
  const ttsDir = join(sessionPath, 'segments', 'tts');
  const tmpDir = join(sessionPath, 'tmp');
  const stretchedDir = join(sessionPath, 'segments', 'stretched');
  const metadataDir = join(sessionPath, 'metadata');

  ensureDir(tmpDir, 'Merge Audio', taskId);
  ensureDir(stretchedDir, 'Merge Audio', taskId);
  ensureDir(metadataDir, 'Merge Audio', taskId);

  const dubbingFile = join(tmpDir, 'audio_dubbing.wav');
  const timingsFile = join(metadataDir, 'timings.json');
  if (!existsSync(timingsFile)) throw new Error(`timings.json not found: ${timingsFile}`);

  const data = readJson(timingsFile, 'Merge Audio', taskId);
  const translation = data.translation;
  const ttsFiles = translation.map((_: any, i: number) => join(ttsDir, `${String(i + 1).padStart(4, '0')}.wav`));

  for (const f of ttsFiles) {
    if (!existsSync(f)) throw new Error(`Missing TTS segment: ${f}`);
  }

  const probe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'stream=sample_rate', '-of', 'csv=p=0', ttsFiles[0]], { stdio: ['pipe', 'pipe', 'pipe'] });
  const sampleRate = parseInt(probe.stdout.toString().trim()) || 48000;

  const segmentInputs: string[] = [];
  let lastEndMs = 0;
  let drift = 0;

  for (let i = 0; i < translation.length; i++) {
    const segment = translation[i];
    const ttsFile = ttsFiles[i];
    const idx = String(i + 1).padStart(4, '0');
    const stretchedFile = join(stretchedDir, `${idx}.wav`);

    const realStartMs = Math.max(segment.start_time, lastEndMs);

    if (realStartMs > lastEndMs) {
      const gapSec = (realStartMs - lastEndMs) / 1000;
      const silenceFile = join(tmpDir, `silence_${i}.wav`);
      if (!existsSync(silenceFile)) {
        ffmpeg(['-f', 'lavfi', '-i', `anullsrc=r=${sampleRate}:cl=mono`, '-t', String(gapSec), silenceFile]);
      }
      segmentInputs.push(silenceFile);
    }

    // Probe original TTS duration
    const ttsDurProbe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', ttsFile], { stdio: ['pipe', 'pipe', 'pipe'] });
    const ttsSec = parseFloat(ttsDurProbe.stdout.toString().trim()) || 0;

    // Trim trailing silence only (areverse so internal pauses aren't mistaken for tail)
    const trimmedFile = join(stretchedDir, `${idx}_trimmed.wav`);
    ffmpeg(['-i', ttsFile, '-af',
      'areverse,silenceremove=start_periods=1:start_threshold=-50dB:start_duration=0.05,areverse',
      trimmedFile]);

    const durProbe = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', trimmedFile], { stdio: ['pipe', 'pipe', 'pipe'] });
    const trimmedSec = parseFloat(durProbe.stdout.toString().trim()) || 0;

    const originalSlotSec = (segment.end_time - realStartMs) / 1000;
    // floor at 50ms so speed calc never goes negative
    const slotSec = Math.max(0.05, originalSlotSec + drift);

    let stretchedSec: number;
    let newDrift: number;

    if (trimmedSec <= slotSec) {
      stretchedSec = trimmedSec;
      newDrift = slotSec - trimmedSec;
      ffmpeg(['-i', trimmedFile, '-c', 'copy', stretchedFile]);
    } else {
      const speed = Math.min(1.05, trimmedSec / slotSec);
      stretchedSec = trimmedSec / speed;
      newDrift = speed < 1.05 ? 0 : slotSec - stretchedSec;
      ffmpeg(['-i', trimmedFile, '-filter:a', `rubberband=tempo=${speed.toFixed(4)}`, stretchedFile]);
    }

    drift = newDrift;
    segmentInputs.push(stretchedFile);

    const realEndMs = Math.floor(realStartMs + stretchedSec * 1000);
    lastEndMs = realEndMs;

    segment.actual_start_time = Math.floor(realStartMs);
    segment.actual_end_time = realEndMs;
    segment.tts_duration_ms = Math.round(ttsSec * 1000);
    segment.stretched_duration_ms = Math.round(stretchedSec * 1000);
  }

  if (segmentInputs.length === 0) throw new Error('No audio segments to merge');

  const concatFile = join(tmpDir, 'concat_list.txt');
  writeFile(concatFile, segmentInputs.map(f => `file '${f}'`).join('\n'), 'Merge Audio', taskId);
  ffmpeg(['-f', 'concat', '-safe', '0', '-i', concatFile, '-acodec', 'pcm_s16le', '-ar', String(sampleRate), '-ac', '1', dubbingFile]);

  writeJson(timingsFile, { translation }, 'Merge Audio', taskId);
  await updateStageDB(taskId, 'merge_audio', { status: 'succeeded', completed_at: nowISO(), progress: 100, last_message: 'Merged' });
}
