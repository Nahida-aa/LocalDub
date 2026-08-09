import { spawn, spawnSync } from "node:child_process";
import { readJson, removeFile } from "@repo/core/utils/fileOps";
import { copyFileSync, existsSync, renameSync } from "node:fs";
import { delimiter, join, resolve, basename } from "node:path";
import { homedir } from "node:os";
import { writeJson, ensureDir } from "@repo/util/file_op";
import {
  emitLog,
  ffmpeg,
  nowISO,
  readTaskLanguages,
  video_source_path,
  vocalsPath,
  mixedVocalsPath,
  gatedVocalsPath,
} from "@repo/core/stages/utils/utils.ts";
import { ensureWhisperCpp, ensureVadModel, whisperCppBinaryPath } from "../ensure.ts";
import { TaskCtx, setCtx, setStage } from "@repo/core/context/context.ts";
import { pythonBin } from "@repo/config/path/bin";
import { REPO_ROOT } from "@repo/config/root";
import { whisperCppModelPath } from "@repo/config/path/models";
import { srtTime } from "@repo/core/utils/utils";

const VAD_CANDIDATES: Record<string, string[]> = {
  "silero-v5": ["silero-v5.1.2", "silero-vad-v5"],
  "silero-v6": ["silero-v6.2.0", "silero-vad-v6"],
};

const VAD_SEARCH_DIRS: string[] = [
  join(homedir(), ".cache", "pywhispercpp"),
  join(REPO_ROOT, "submodule", "whisper.cpp", "models"),
];

function resolveVadModel(name: string): string {
  const candidates = VAD_CANDIDATES[name];
  if (!candidates) return name;
  for (const dir of VAD_SEARCH_DIRS) {
    for (const c of candidates) {
      const p = join(dir, `ggml-${c}.bin`);
      if (existsSync(p)) return p;
      // fallback: allow versioned filenames like ggml-silero-vad-v6.2.0.bin
      try {
        const files = require("node:fs").readdirSync(dir);
        for (const f of files) {
          if (!f.startsWith("ggml-") || !f.endsWith(".bin")) continue;
          if (f.includes(c) || f.includes(c.split(".")[0])) return join(dir, f);
        }
      } catch (e) {
        // ignore and continue
      }
    }
  }
  const dir = join(REPO_ROOT, "submodule", "whisper.cpp", "models");
  throw new Error(
    `VAD model '${name}' not found. Install via:\n` +
      `  bash ${dir}/download-vad-model.sh ${candidates[0]}\n` +
      `Expected locations: ${VAD_SEARCH_DIRS.map((d) => `\n  - ${d}/ggml-{${candidates.join(",")}}.bin`).join("")}`,
  );
}

export async function asrWhisperCpp(
  ctx: TaskCtx,
  audioPath: string,
  taskDir: string,
  language: string = "auto",
) {
  const whisperCli = whisperCppBinaryPath();
  const model = whisperCppModelPath();

  emitLog(taskDir, `[asr] runtime=ggml binary=${whisperCli}`);

  if (!ensureWhisperCpp(taskDir)) {
    throw new Error("whisper.cpp setup failed; see logs above for manual steps");
  }
  if (ctx.input?.stages?.asr?.vad && ctx.input?.stages?.asr?.vadModel) {
    ensureVadModel(taskDir);
  }

  // whisper-cli writes <audioPath>.json alongside input; place input in the persistent asr directory for inspection
  const audioDir = join(taskDir, "asr");
  ensureDir(audioDir);
  // Prepare input WAV for whisper-cli: if input is already WAV, use it directly to avoid unnecessary copies
  let tmpAudio: string;
  if (audioPath.toLowerCase().endsWith(".wav")) {
    tmpAudio = audioPath;
    emitLog(taskDir, `[ASR] Using existing WAV input: ${tmpAudio}`);
  } else {
    // converted WAV will be placed under session asr directory
    tmpAudio = join(audioDir, "whisper-input.wav");
    spawnSync("ffmpeg", ["-y", "-i", audioPath, "-ac", "1", tmpAudio], {
      timeout: 30_000,
    });
    if (!existsSync(tmpAudio)) {
      throw new Error(`ffmpeg failed to convert ${audioPath} to ${tmpAudio}`);
    }
  }

  const t0 = performance.now();
  const whisperArgs = ["-m", model, tmpAudio, "-l", language, "-t", "4", "-ojf"];
  const asrCfg = ctx.input?.stages?.asr;
  if (asrCfg?.vad) {
    whisperArgs.push("--vad");
    if (asrCfg.vadModel) whisperArgs.push("-vm", resolveVadModel(asrCfg.vadModel));
  }
  ["vadThreshold", "noSpeechThold", "temperature"].forEach((k) => {
    const v = (asrCfg as any)?.[k];
    if (v !== undefined && v !== null) {
      whisperArgs.push(`--${k.replace(/([A-Z])/g, "-$1").toLowerCase()}`, String(v));
    }
  });
  if (asrCfg?.maxLen && asrCfg.maxLen > 0) whisperArgs.push("--max-len", String(asrCfg.maxLen));
  if (asrCfg?.splitOnWord) whisperArgs.push("--split-on-word");
  const libPathKey = (() => {
    if (process.platform !== "win32") return "LD_LIBRARY_PATH";
    const existing = Object.keys(process.env).find((k) => k.toLowerCase() === "path");
    return existing || "PATH";
  })();
  const { dirname } = await import("node:path");
  const { readdirSync } = await import("node:fs");
  const binDir = dirname(whisperCli);

  // Determine candidate Release directories robustly to avoid 'Release/Release' when the
  // binary already sits in a Release folder or when DLLs live under build/bin/Release.
  const repoWhisperBuild = join(REPO_ROOT, "submodule", "whisper.cpp", "build");
  const candidateReleaseDirs = [
    join(binDir, "Release"), // common when binary in build/bin
    join(binDir, "..", "Release"), // common when binary in build/Release
    join(repoWhisperBuild, "bin", "Release"),
    join(repoWhisperBuild, "Release"),
  ];
  const existingReleaseDirs = candidateReleaseDirs.filter((d) => existsSync(d));
  const chosenReleaseDir = existingReleaseDirs.length
    ? existingReleaseDirs[0]
    : candidateReleaseDirs[0];

  // Log chosen binary & availability for diagnostics
  emitLog(
    taskDir,
    `[ASR] whisper binary chosen=${whisperCli} exists=${existsSync(whisperCli)} binDir=${binDir} releaseDir=${chosenReleaseDir} releaseExists=${existsSync(chosenReleaseDir)}`,
  );
  try {
    const binFiles = existsSync(binDir) ? readdirSync(binDir).join(",") : "";
    emitLog(taskDir, `[ASR] binDir files=${binFiles}`);
    if (existsSync(chosenReleaseDir)) {
      emitLog(taskDir, `[ASR] releaseDir files=${readdirSync(chosenReleaseDir).join(",")}`);
    }
  } catch (e) {
    // ignore listing errors
  }

  // Build lib path list: include binDir and any existing Release dirs (deduplicated)
  const releaseDirsToInclude = Array.from(
    new Set([
      binDir,
      ...existingReleaseDirs,
      join(binDir, "..", "src"),
      join(binDir, "..", "ggml", "src"),
      join(binDir, "..", "ggml", "src", "ggml-hip"),
    ]),
  );

  const result = spawnSync(whisperCli, whisperArgs, {
    timeout: 600_000,
    env: {
      ...process.env,
      [libPathKey]: [
        // include build bin and Release folders so DLLs (ggml/*.dll, whisper.dll) are found on Windows
        ...releaseDirsToInclude,
        process.env[libPathKey] || "",
      ]
        .filter(Boolean)
        .join(delimiter),
    },
  });

  // Diagnostic logging on failure
  if (result.status !== 0 && result.status !== null) {
    emitLog(
      taskDir,
      `[ASR] whisper-cli exit=${result.status} stdout=${result.stdout?.toString().slice(-2000) ?? ""} stderr=${result.stderr?.toString().slice(-2000) ?? ""}`,
    );
    emitLog(taskDir, `[ASR] PATH used=${(process.env[libPathKey] || "").slice(-2000)}`);
  }

  const elapsedSec = (performance.now() - t0) / 1000;

  if (result.status !== 0 && result.status !== null) {
    throw new Error(
      `whisper-cli failed (${result.status}): ${result.stderr?.toString().slice(-300)}`,
    );
  }

  // Read the generated JSON
  let whisperJson = `${tmpAudio}.json`;
  if (!existsSync(whisperJson)) {
    throw new Error(`whisper-cli did not produce ${whisperJson}`);
  }

  const raw = await readJson(whisperJson, ctx);
  // Move the generated whisper JSON into the session asr directory for centralized storage
  const destWhisperJson = join(audioDir, basename(whisperJson));
  try {
    if (whisperJson !== destWhisperJson && existsSync(whisperJson)) {
      renameSync(whisperJson, destWhisperJson);
      emitLog(taskDir, `[ASR] Moved ${whisperJson} -> ${destWhisperJson}`);
      // update whisperJson path to the new location for downstream use
      whisperJson = destWhisperJson;
    }
  } catch (e) {
    emitLog(taskDir, `[ASR] Failed to move whisper json: ${(e as any)?.message ?? e}`);
  }
  const detected_language = raw.result.language ?? "auto";
  setCtx(taskDir, { asr_language: detected_language });
  const transcription: any[] = raw.transcription || [];

  const emitWords = ctx.input?.stages?.asr?.wordsOutput ?? true;

  const segments = transcription.map((s: any) => {
    const startMs = s.offsets?.from ?? 0;
    const endMs = s.offsets?.to ?? 0;
    const seg: Record<string, any> = {
      text: (s.text || "").trim(),
      start: startMs,
      end: endMs,
      start_fmt: srtTime(startMs),
      end_fmt: srtTime(endMs),
    };
    if (emitWords) {
      const rawWords = (s.tokens || [])
        .filter((t: any) => {
          const txt = t.text?.trim();
          return txt && !txt.startsWith("[") && t.offsets?.from != null;
        })
        .map((t: any) => ({
          word: t.text.trim(),
          start: t.offsets.from ?? 0,
          end: t.offsets.to ?? 0,
          probability: t.p,
        }));
      if (rawWords.length > 0) {
        const offset = startMs - rawWords[0].start;
        if (Math.abs(offset) > 500) {
          emitLog(
            taskDir,
            `[ASR] VAD word timestamp shift: ${rawWords.length} words offset by ${Math.round(offset)}ms`,
          );
        }
        for (const w of rawWords) {
          w.start += offset;
          w.end += offset;
        }
      }
      seg.words = rawWords;
      const probs = rawWords.map((w: any) => w.probability).filter((p: number) => p >= 0);
      if (probs.length > 0) {
        seg.confidence = {
          avg: +(probs.reduce((a: number, b: number) => a + b, 0) / probs.length).toFixed(4),
          min: +Math.min(...probs).toFixed(4),
        };
      }
    }
    return seg;
  });
  const text = segments.map((s) => s.text).join(" ");

  const asrDir = resolve(taskDir, "asr");
  ensureDir(asrDir);

  const lastEndMs = segments.length ? segments[segments.length - 1].end : 0;
  const asrOutput = {
    audio_info: { duration: lastEndMs },
    result: { text, segments },
    _engine: "whisper.cpp",
    _model: model,
    _params: Object.fromEntries(Object.entries(asrCfg ?? {}).filter(([, v]) => v !== undefined)),
    _input_audio: audioPath,
    _device: "vulkan",
    _rtf: elapsedSec > 0 && lastEndMs > 0 ? (elapsedSec / (lastEndMs / 1000)).toFixed(3) : "0",
    detected_language,
  };
  writeJson(join(asrDir, "asr.json"), asrOutput);

  // Preserve whisper input and json in asr directory for debugging and auditing
  emitLog(taskDir, `[ASR] Preserving ${tmpAudio} and ${whisperJson} in asr directory`);

  emitLog(taskDir, `[ASR] Transcribed in ${elapsedSec.toFixed(1)}s`);
  if (segments.length > 0) {
    const audioDurationMs = segments[segments.length - 1].end;
    emitLog(taskDir, `[ASR] Audio duration ${(audioDurationMs / 1000).toFixed(1)}s`);
    emitLog(taskDir, `[ASR] RTF ${(elapsedSec / (audioDurationMs / 1000)).toFixed(3)}`);
  }
}
