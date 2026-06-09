import { spawnSync } from 'node:child_process';
import { existsSync, copyFileSync, writeFileSync, mkdirSync, unlinkSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { Demucs } from '../../cli/src/ml/demucs/demucs';
import { RESULTS_DIR, audioDuration, round } from './bench-shared';

const REPO_ROOT = resolve(__dirname, '..', '..', '..');
const VIDEO_PATH = join(REPO_ROOT, 'packages', 'benchmark', 'video_source.mp4');
const TMP_WAV = join(REPO_ROOT, 'packages', 'tmp', 'demucs-ort-video.wav');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'asr_manual.json');
const PYTHON_BIN = join(REPO_ROOT, '.venv', 'bin', 'python');
const WER_PY = join(__dirname, 'wer.py');
const WHISPER_CLI = join(REPO_ROOT, 'submodule', 'whisper.cpp', 'build', 'bin', 'whisper-cli');
const MODEL = process.env.WHISPER_MODEL || join(process.env.HOME || '/root', '.cache', 'pywhispercpp', 'ggml-large-v3-turbo.bin');

function extractAudio(): number {
  console.log('[ORT] Extracting audio...');
  const t0 = performance.now();
  const r = spawnSync('ffmpeg', [
    '-y', '-i', VIDEO_PATH, '-vn', '-ac', '2', '-ar', '44100', TMP_WAV,
  ], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`ffmpeg failed: ${r.stderr?.toString().slice(-200)}`);
  const dur = audioDuration(TMP_WAV);
  console.log(`  Duration: ${dur.toFixed(1)}s (extract: ${((performance.now() - t0) / 1000).toFixed(1)}s)`);
  return dur;
}

function transcribeWithWhisper(wavPath: string): string {
  const result = spawnSync(WHISPER_CLI, ['-m', MODEL, wavPath, '-l', 'zh', '-t', '4', '-ng'], {
    timeout: 600_000,
    env: {
      ...process.env,
      LD_LIBRARY_PATH: [
        join(REPO_ROOT, 'submodule', 'whisper.cpp', 'build', 'src'),
        join(REPO_ROOT, 'submodule', 'whisper.cpp', 'build', 'ggml', 'src'),
        join(REPO_ROOT, 'submodule', 'whisper.cpp', 'build', 'ggml', 'src', 'ggml-hip'),
        process.env.LD_LIBRARY_PATH || '',
      ].filter(Boolean).join(':'),
    },
  });

  if (result.status !== 0) throw new Error(`whisper-cli failed: ${result.stderr?.toString().slice(-300)}`);

  // Parse segments from stdout
  const segments: any[] = [];
  for (const line of result.stdout.toString().split('\n')) {
    const m = line.match(/^\[(\d+):(\d+):(\d+)\.(\d+)\s*-->\s*(\d+):(\d+):(\d+)\.(\d+)\]\s+(.*)/);
    if (m) {
      segments.push({
        text: m[9].trim(),
        start: parseInt(m[1]) * 3600 + parseInt(m[2]) * 60 + parseInt(m[3]) + parseInt(m[4]) / 100,
        end: parseInt(m[5]) * 3600 + parseInt(m[6]) * 60 + parseInt(m[7]) + parseInt(m[8]) / 100,
      });
    }
  }

  const text = segments.map((s: any) => s.text).join(' ');
  writeFileSync(join(RESULTS_DIR, 'wer-ort-video.json'), JSON.stringify({
    audio_info: { duration: 0 },
    result: { text, segments },
    _device: 'cpu',
  }, null, 2));

  return text;
}

function computeWER(hypFile: string): { wer: number; cer: number } {
  const r = spawnSync(PYTHON_BIN, [WER_PY, GROUND_TRUTH, hypFile], { timeout: 30_000 });
  if (r.status !== 0) throw new Error(`wer.py failed: ${r.stderr?.toString().slice(-200)}`);
  return JSON.parse(r.stdout.toString());
}

async function main() {
  mkdirSync(RESULTS_DIR, { recursive: true });

  if (!existsSync(VIDEO_PATH)) {
    console.error(`Video not found: ${VIDEO_PATH}`);
    process.exit(1);
  }

  const durS = extractAudio();

  // Load model
  console.log('[ORT] Loading model...');
  const t0 = performance.now();
  const demucs = new Demucs();
  await demucs.load();
  const loadTimeS = (performance.now() - t0) / 1000;
  console.log(`  Load: ${loadTimeS.toFixed(1)}s`);

  // Separate
  console.log('[ORT] Separating...');
  const t1 = performance.now();
  const stems = await demucs.separate(TMP_WAV);
  const totalTimeS = (performance.now() - t1) / 1000;
  const processTimeS = totalTimeS;
  const rtf = round(processTimeS / durS, 3);
  console.log(`  Process: ${processTimeS.toFixed(1)}s, RTF: ${rtf}`);

  // Save vocals
  const vocalsPath = join(RESULTS_DIR, 'video-ort-vocals.wav');
  demucs.writeWav(stems.vocals, stems.sampleRate, vocalsPath);
  console.log(`  Vocals saved: ${vocalsPath}`);

  // Transcribe with whisper
  console.log('[ORT] Transcribing with whisper...');
  transcribeWithWhisper(vocalsPath);

  // Compute WER/CER
  const hypFile = join(RESULTS_DIR, 'wer-ort-video.json');
  const { wer, cer } = computeWER(hypFile);
  console.log(`  WER: ${(wer * 100).toFixed(2)}%, CER: ${(cer * 100).toFixed(2)}%`);

  // Summary
  console.log('\n=== ORT Video Result ===');
  console.log(`RTF: ${rtf}`);
  console.log(`WER: ${(wer * 100).toFixed(2)}%`);
  console.log(`CER: ${(cer * 100).toFixed(2)}%`);

  // Save summary to results
  writeFileSync(join(RESULTS_DIR, 'ort-video-summary.json'), JSON.stringify({
    engine: 'ort',
    device: 'cpu',
    durationS: round(durS, 3),
    loadTimeS: round(loadTimeS, 3),
    processTimeS: round(processTimeS, 3),
    totalTimeS: round(totalTimeS + loadTimeS, 3),
    rtf,
    wer: round(wer, 4),
    cer: round(cer, 4),
  }, null, 2));

  // Cleanup
  if (existsSync(TMP_WAV)) unlinkSync(TMP_WAV);
}

main().catch(err => {
  console.error('Failed:', err);
  process.exit(1);
});
