import { spawn, spawnSync } from "node:child_process";
import { readJson, removeFile } from "@repo/core/utils/fileOps";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { copyFileSync, existsSync, renameSync } from "node:fs";
import { delimiter, join, resolve, basename } from "node:path";
import { homedir } from "node:os";

import {
  emitLog,
  ffmpeg,
  nowISO,
  readTaskLanguages,
  video_source_path,
  vocalsPath,
  mixedVocalsPath,
  gatedVocalsPath,
} from "@repo/core/stages/utils/utils";
import { TaskCtx, setCtx, setStage } from "@repo/core/context/context.ts";
import { pythonBin } from "@repo/config/path/bin";
import { asrWhisperCpp } from "../../ml/whisper/runtime/ggml.ts";
import { AsrResult } from "@repo/subtitle-asr/types";
import { log } from "@repo/util/log";

export async function stageAsr(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  await setStage(taskDir, "asr", {
    last_message: "Transcribing...",
    progress: 0,
  });
  const audioVocal = ctx.input?.stages?.asr?.vocalAudioPath ?? vocalsPath(taskDir);
  const videoSource = video_source_path(ctx);

  let audioPath = ctx.input?.stages?.asr?.useSeparated ? audioVocal : videoSource;
  if (!existsSync(audioPath))
    throw new Error(
      `ASR input not found: ${audioPath}; 如果 asr.useSeparated=true, 请确保 target_3_vocals.wav 存在；如果 asr.useSeparated=false, 请确保 video_source.mp4 存在`,
    );

  if (ctx.input?.stages?.asr?.useSeparated) {
    const mixedPath = mixedVocalsPath(taskDir);
    const gatedPath = gatedVocalsPath(taskDir);
    const mixedOrGated = existsSync(gatedPath)
      ? gatedPath
      : existsSync(mixedPath)
        ? mixedPath
        : null;
    if (mixedOrGated) {
      audioPath = mixedOrGated;
      emitLog(taskDir, `[ASR] Using pre-mixed audio: ${mixedOrGated}`);
    } else {
      emitLog(taskDir, `[ASR] No mixed audio found, using vocals-only`);
    }
  }

  const asrCfg = ctx.input?.stages?.asr;
  const runtime = asrCfg?.runtime ?? "pytorch";
  const device = asrCfg?.device ?? "cuda";
  emitLog(taskDir, `[ASR] runtime=${runtime} device=${device}`);

  const pyBin = pythonBin();
  ctx.input.task.sourceLang;

  // if (runtime === "pytorch") {
  //   emitLog(taskDir, `[ASR] Using demucs_torch_server (device=${device})`);
  //   const { port } = await findServer("demucs_torch_server");
  //   const asrUrl = getTorchServerUrl(port);
  //   const result = await runStage(asrUrl, "asr", taskId, {
  //     vocals_path: audioPath,
  //     task_dir: taskDir,
  //     language: ctx.input.task.sourceLang || "auto",
  //     device,
  //     word_timestamps: asrCfg?.wordsOutput ?? false,
  //   });
  //   const r = result as Record<string, any>;
  //   const actualDevice: string = r.actual_device ?? device;
  //   const fallbackToCpu = device !== "cpu" && actualDevice === "cpu";
  //   if (fallbackToCpu) {
  //     console.warn(`[WARN] [ASR] GPU failed, fell back to CPU (actual device: ${actualDevice})`);
  //   }
  //   if (r.detected_language) {
  //     setCtx(taskDir, {
  //       runInfo: {
  //         asr: {
  //           engine: "whisper-pytorch",
  //           device: actualDevice,
  //           gpuAttempted: device !== "cpu",
  //           fallbackToCpu,
  //         },
  //       },
  //     });
  //   }
  //   if (r.load_time_s) emitLog(taskDir, `[ASR] Model loaded in ${r.load_time_s}s`);
  //   if (r.process_time_s) emitLog(taskDir, `[ASR] Transcribed in ${r.process_time_s}s`);
  //   if (r.audio_duration_s)
  //     emitLog(taskDir, `[ASR] Audio duration ${Number(r.audio_duration_s).toFixed(1)}s`);
  //   if (r.rtf) emitLog(taskDir, `[ASR] RTF ${r.rtf}`);
  // } else
  if (runtime === "ggml") {
    await asrWhisperCpp(ctx, audioPath, taskDir, ctx.input.task.sourceLang);
  } else {
    await asrWhisperCpp(ctx, audioPath, taskDir, ctx.input.task.sourceLang);
  }

  /**
   * asr 后处理, 避免 asr_fix 拿到多余数据
   */
  const asrFile = join(taskDir, "asr", "asr.json");
  if (!existsSync(asrFile)) {
    throw new Error(`ASR file not found: ${asrFile}`);
  }
  const data = await readJson<AsrResult>(asrFile);
  setCtx(taskDir, {
    asr_language: data.meta.detected_language,
  });
  // 统一过滤超出音频时长的幻觉段（所有路径 shared）
  const durationMs = data.meta.audio_duration ?? 0;
  if (durationMs > 0 && data.result?.segments?.length) {
    const before = data.result.segments.length;
    data.result.segments = data.result.segments.filter(
      (u) => u.start_ms < durationMs && u.end_ms > 0,
    );
    if (data.result.segments.length < before) {
      const removed = before - data.result.segments.length;
      log(`Removed ${removed} hallucinated segment(s) (start >= ${durationMs}ms or end <= 0ms)`);
      writeJson(asrFile, data);
    }
  }

  // 能量检测：最后一个 segment 如果 RMS 过低则判为幻觉
  const last = data.result?.segments?.[data.result.segments.length - 1];
  if (last && existsSync(audioPath)) {
    const rms = await segmentRms(audioPath, last.start_ms, last.end_ms);
    console.log(`[ASR] Last segment RMS: ${rms}`);
    if (rms > 0 && rms < 0.005) {
      const removed = data.result.segments.pop();
      if (removed) {
        log(
          `Removed low-energy hallucinated segment "${removed.text.slice(0, 30)}" (RMS=${rms.toFixed(5)})`,
        );
        writeJson(asrFile, data);
      }
    }
  }

  await setStage(taskDir, "asr", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: "Transcribed",
  });
}

function segmentRms(audioPath: string, startMs: number, endMs: number): Promise<number> {
  return new Promise((resolve, reject) => {
    const args = [
      "-y",
      "-i",
      audioPath,
      "-ss",
      String(startMs / 1000),
      "-to",
      String(endMs / 1000),
      "-af",
      "astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-",
      "-f",
      "null",
      "-",
    ];
    const proc = spawn("ffmpeg", args, { timeout: 30_000 });
    let stderr = "";
    proc.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    proc.on("close", (code) => {
      if (code !== 0) return resolve(0);
      const m = stderr.match(/lavfi\.astats\.Overall\.RMS_level=([-\d.]+)/);
      if (!m) return resolve(0);
      const dB = parseFloat(m[1]);
      // dB → linear: linear = 10^(dB/20)
      resolve(Math.pow(10, dB / 20));
    });
    proc.on("error", () => resolve(0));
  });
}
