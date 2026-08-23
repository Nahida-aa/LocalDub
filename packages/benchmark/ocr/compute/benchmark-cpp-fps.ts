import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { mergeFrames } from "@repo/subtitle-ocr/ocr_fix/merge_frames";
import { findRapidOcrModelsDir } from "@repo/subtitle-ocr/utils";

const REPO_ROOT = resolve(__dirname, "..", "..", "..", "..");
const VIDEO = resolve(REPO_ROOT, "packages", "benchmark", "ref", "video_source.mp4");
const OCR_KEYS_PATH = resolve(REPO_ROOT, "packages", "subtitle-ocr", "ppocr_keys.json");
const OCR_MODELS_DIR = findRapidOcrModelsDir();
const CPP = resolve(
  REPO_ROOT,
  "packages",
  "subtitle-ocr",
  "ort-cpp",
  "build",
  "subtitle_ocr_ort_cpp",
);
const LD = resolve(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp", "build");
const OUT = resolve(REPO_ROOT, "packages/benchmark/ocr/results/ort-cpp-2fps-so-ts0.45");
const TMP = resolve(REPO_ROOT, "packages", "tmp", "ocr-2fps");
const FPS = 2;
const TEXT_SCORE = 0.45;
const SUBTITLE_ONLY = true;

mkdirSync(join(OUT, "metadata"), { recursive: true });
mkdirSync(TMP, { recursive: true });

// probe duration
const probe = spawnSync(
  "ffprobe",
  ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", VIDEO],
  { encoding: "utf-8" },
);
const dur = Math.ceil(parseFloat(probe.stdout.trim()));
console.log(`Duration: ${dur}s @ ${FPS}fps = ${dur * FPS} frames`);

// extract frames at exact timestamps
const totalFrames = Math.ceil(dur * FPS);
const frameFiles: string[] = [];
for (let i = 0; i < totalFrames; i++) {
  const ts = (i * 1000) / FPS;
  const fname = `frame_${String(i + 1).padStart(5, "0")}.jpg`;
  const fp = join(TMP, fname);
  spawnSync(
    "ffmpeg",
    ["-y", "-ss", String(ts / 1000), "-i", VIDEO, "-frames:v", "1", "-qscale:v", "2", fp],
    { timeout: 10_000, stdio: "ignore" },
  );
  frameFiles.push(fname);
}
console.log(`Extracted ${frameFiles.length} frames at exact ${FPS}fps intervals`);

// OCR each frame
const frames: { text: string; timestamp: number }[] = [];
for (let i = 0; i < frameFiles.length; i++) {
  const fp = join(TMP, frameFiles[i]);
  const ts = Math.round((i / FPS) * 1000);
  const args = [fp, String(TEXT_SCORE)];
  if (SUBTITLE_ONLY) args.push("--subtitle-only");
  const r = spawnSync(CPP, args, {
    timeout: 60_000,
    encoding: "utf-8",
    env: { ...process.env, LD_LIBRARY_PATH: LD, OCR_MODELS_DIR, OCR_KEYS_PATH },
  });
  if (r.status !== 0) {
    frames.push({ text: "", timestamp: ts });
    continue;
  }
  try {
    const parsed = JSON.parse(r.stdout);
    const item = (Array.isArray(parsed) ? parsed[0] : parsed) || {};
    frames.push({ text: item.text || "", timestamp: ts });
  } catch {
    frames.push({ text: "", timestamp: ts });
  }
  if ((i + 1) % 50 === 0) console.log(`  OCR: ${i + 1}/${frameFiles.length}`);
}

const { segments: segs, text } = mergeFrames(
  frames.map((f) => ({ text: f.text, timestamp: f.timestamp, confidence: 0 })),
  {},
);

// filter short segments (< 500ms)
const filtered = segs.filter((s) => s.end - s.start >= 500);
console.log(
  `${frameFiles.length} frames → ${segs.length} segs → ${filtered.length} after min-duration filter`,
);

const ocrOutput = {
  audio_info: { duration: frames.length > 0 ? frames[frames.length - 1].timestamp : 0 },
  result: { text, segments: filtered },
};
writeFileSync(join(OUT, "metadata", "ocr.json"), JSON.stringify(ocrOutput, null, 2));
console.log(
  `Written: ${join(OUT, "metadata", "ocr.json")} (${filtered.length} segs, ${text.length} chars)`,
);

spawnSync("rm", ["-rf", TMP]);

const rel = join(
  "packages",
  "benchmark",
  "ocr",
  "results",
  "ort-cpp-2fps-so-ts0.45",
  "metadata",
  "ocr.json",
);
console.log(
  `\nEval: bun run packages/benchmark/ref/compute/eval-ocr.ts ${rel} --label ort-cpp-2fps-so-ts0.45 --ms`,
);
