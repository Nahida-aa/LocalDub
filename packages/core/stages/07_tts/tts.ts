import { readJson, writeFile, writeFileSync, rmSync } from "@repo/core/utils/fileOps";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { existsSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { writeWav } from "@repo/voxlab";
import type { TtsFile, TtsSegment } from "./out.ts";

import {
  ffmpeg,
  nowISO,
  read_split_audio_timings,
  tts_filepath,
} from "@repo/core/stages/utils/utils.ts";
import { probeDurationMs } from "@repo/core/utils/ffmpeg";
import { TaskCtx, setStage, setTask } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { newVoxCPMEngine } from "@repo/core/ml/voxcpm/voxcpm";
import { log } from "@repo/util/log";

// vocals 参考音的"非静音"判定阈值: PCM 裸数据 > 该字节数才认为有实际声音内容。
// 1200 = 1200 个采样帧 (约 75ms @ 16kHz), 16 = 16bit 采样深度, 2 = 双声道。
const MIN_REF_BYTES = 1200 * 16 * 2;
// refAudioX2 触发阈值: 参考音短于该时长时, 拼接自身翻倍作为 TTS 参考输入。
const MIN_REF_DURATION_MS = 2500;

/**
 * Progress bar
 */
function renderProgress(current: number, total: number, start: number) {
  const elapsed = (Date.now() - start) / 1000;
  const frac = total > 0 ? current / total : 0;
  const pct = (frac * 100).toFixed(0).padStart(3);
  const barW = 10;
  const fracW = frac * barW;
  const fill = Math.min(Math.floor(fracW), barW);
  const blockChars = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
  const barFull = "█".repeat(fill);
  const barRest = fill >= barW ? "" : blockChars[Math.round((fracW - fill) * 8)] || " ";
  const barEmpty = fill >= barW ? "" : " ".repeat(Math.max(0, barW - fill - 1));
  const bar = `${barFull}${barRest}${barEmpty}`;
  const rate = current > 0 ? current / elapsed : 0;
  const eta = total > 0 && rate > 0 ? (total - current) / rate : 0;
  const fmt = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
  };
  process.stdout.write(
    `\r${pct}%|${bar}| ${current}/${total} [${fmt(elapsed)}<${fmt(eta)}, ${rate.toFixed(2)}it/s]`,
  );
}

export async function stageTts(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  startLog(taskDir, taskId);

  const ttsArgs = ctx.input.stages.tts;
  const vocalsDir = join(taskDir, "split_audio", "vocals");
  const ttsWavDir = join(taskDir, "tts", "wavs");
  const doubledDir = join(taskDir, "tts", "ref_doubled");

  ensureDir(ttsWavDir);
  if (ttsArgs.refAudioX2) {
    ensureDir(doubledDir);
  }

  const { segments } = await read_split_audio_timings(ctx);

  if (!ttsArgs.skipExisting) {
    const anyTts = readdirSync(ttsWavDir).find((f) => f.endsWith(".wav"));
    if (anyTts) {
      log(`Existing TTS segments found; will overwrite without deleting files`);
    }
  }
  // Unified engine (handles all runtimes via createBackend)

  log(`Using ${ttsArgs.runtime} backend`);
  const engine = newVoxCPMEngine(ttsArgs);
  await engine.load();

  //  Generation loop
  const tqdmStart = Date.now();
  let generated = 0,
    skipped = 0,
    errors = 0;
  let genMs = 0;
  const ttsSegments: TtsSegment[] = [];

  // Find fallback reference for segments without usable reference audio
  /**
   * 遍历 1 到 translation.length 个 segment 的 vocals 文件（0001.wav ~ 000N.wav），找到第一个非静音的作为 fallbackRef。
   * existsSync(refPath) && statSync(refPath).size > 1200 * 16 * 2 这个阈值：
   * - 1200 = 1200 个采样帧（约 75ms @ 16kHz）
   * - 16 = 16bit 采样深度
   * - 2 = 双声道
   * - 即 PCM 裸数据 > 38400 bytes 才认为有实际声音内容
   * 目的：后面如果有 segment 没有对应的 vocals 文件（或 vocals 太短是静音），就用这个 fallbackRef 作为 TTS 的参考音频输入，避免缺参考音导致 TTS 效果差或报错。
   */
  const i = segments.findIndex((_, i) => {
    const refPath = join(vocalsDir, `${String(i + 1).padStart(4, "0")}.wav`);
    return existsSync(refPath) && statSync(refPath).size > MIN_REF_BYTES;
  });
  const fallbackRef = i !== -1 ? join(vocalsDir, `${String(i + 1).padStart(4, "0")}.wav`) : "";

  const isStart = ctx.input?.task.action === "start";
  const regenIndices = isStart ? undefined : ttsArgs.regenIndices;

  let existingSegments: Map<number, TtsSegment> | undefined;
  if (regenIndices?.length) {
    const existingPath = tts_filepath(taskDir);
    if (existsSync(existingPath)) {
      const existing = await readJson<TtsFile>(existingPath);
      existingSegments = new Map(existing.segments.map((s) => [s.seg_idx, s]));
    }
  }

  for (const [i, item] of segments.entries()) {
    const idx = String(i + 1).padStart(4, "0");
    const outPath = resolve(ttsWavDir, `${idx}.wav`);

    if (regenIndices?.length && !regenIndices.includes(i + 1)) {
      // regenIndices 仅作用于「存在有效旧结果」的段: 列表外且旧结果有效才复用并跳过。
      // 没有有效旧结果 (无记录 / wav 缺失 / wav 零时长损坏) 的段, 无论是否在列表里
      // 都必须正常生成, 不能因为不在 regenIndices 中就被跳过复用坏结果。
      const existing = existingSegments?.get(i + 1);
      const oldValid = !!existing && existsSync(outPath) && probeDurationMs(outPath) > 0;
      if (oldValid) {
        ttsSegments.push(existing!);
        skipped += 1;
        renderProgress(i + 1, segments.length, tqdmStart);
        continue;
      }
      // fall through: 走下方空译文/无参考音/正式合成逻辑
    }

    if (regenIndices?.length && existsSync(outPath)) {
      rmSync(outPath, { force: true });
    }

    let refWav = join(vocalsDir, `${idx}.wav`);
    if (!existsSync(refWav) || statSync(refWav).size < MIN_REF_BYTES) {
      refWav = fallbackRef;
    }
    const refMtime = refWav && existsSync(refWav) ? statSync(refWav).mtimeMs : 0;

    if (ttsArgs.skipExisting && existsSync(outPath) && statSync(outPath).mtimeMs > refMtime) {
      const durMs = probeDurationMs(outPath);
      ttsSegments.push({
        seg_idx: i + 1,
        text: item.text,
        dst: item.dst,
        start_ms: item.start_ms,
        end_ms: item.start_ms + durMs,
        slot_end_ms: item.end_ms,
        tts_duration_ms: durMs,
        status: "skipped",
      });
      skipped += 1;
      renderProgress(i + 1, segments.length, tqdmStart);
      continue;
    }

    const text = item.dst || "";
    if (!text.trim()) {
      writeFile(outPath, Buffer.alloc(44), ctx);
      ttsSegments.push({
        seg_idx: i + 1,
        text: "",
        dst: "",
        start_ms: item.start_ms,
        end_ms: item.start_ms,
        slot_end_ms: item.end_ms,
        tts_duration_ms: 0,
        status: "empty",
      });
      skipped += 1;
      renderProgress(i + 1, segments.length, tqdmStart);
      continue;
    }

    if (!refWav || !existsSync(refWav)) {
      log(`[WARN] No reference for segment ${idx}, skipping`);
      writeFile(outPath, Buffer.alloc(44), ctx);
      ttsSegments.push({
        seg_idx: i + 1,
        text: item.text,
        dst: item.dst,
        start_ms: item.start_ms,
        end_ms: item.start_ms,
        slot_end_ms: item.end_ms,
        tts_duration_ms: 0,
        status: "skipped",
      });
      skipped += 1;
      renderProgress(i + 1, segments.length, tqdmStart);
      continue;
    }

    // Double reference audio if shorter than MIN_REF_DURATION_MS
    if (ttsArgs.refAudioX2) {
      const minRefMs = MIN_REF_DURATION_MS;
      const refMs = probeDurationMs(refWav);
      if (refMs > 0 && refMs < minRefMs) {
        const doubled = resolve(doubledDir, `ref_${idx}_x2.wav`);
        if (!existsSync(doubled)) {
          const listPath = resolve(doubledDir, `ref_${idx}_list.txt`);
          writeFileSync(listPath, `file '${refWav}'\nfile '${refWav}'`);
          ffmpeg(["-f", "concat", "-safe", "0", "-i", listPath, "-c", "copy", doubled]);
        }
        refWav = doubled;
      }
    }

    setStage(taskDir, "tts", {
      last_message: `Generating ${i + 1}/${segments.length}...`,
    });
    renderProgress(i + 1, segments.length, tqdmStart);

    const t1 = performance.now();
    let ttsDurationMs = 0;
    let ttsOk = false;
    try {
      const samples = await engine.synthesize(text, refWav, item.text);
      genMs += performance.now() - t1;
      writeWav(samples, outPath, 48000);
      ttsDurationMs = probeDurationMs(outPath);
      generated += 1;
      ttsOk = true;
    } catch (e) {
      log(
        `[tts] [ERROR] Segment ${idx} failed: ${e instanceof Error ? e.message : JSON.stringify(e)}`,
      );
      // 重试逻辑在 engine.synthesize 内部完成; 走到这里说明已重试耗尽仍失败。
      // 直接抛出终止任务, 避免产出零内容音频被后续阶段当作正常段。
      throw new Error(
        `[tts] Segment ${idx} TTS failed after retries: ${e instanceof Error ? e.message : JSON.stringify(e)}`,
      );
    }

    ttsSegments.push({
      seg_idx: i + 1,
      text: item.text,
      dst: item.dst,
      start_ms: item.start_ms,
      end_ms: item.start_ms + ttsDurationMs,
      slot_end_ms: item.end_ms,
      tts_duration_ms: ttsDurationMs,
      status: ttsOk ? "success" : "error",
    });
  }

  await engine.release();
  process.stdout.write("\n");

  const genSec = genMs / 1000;
  const audioSec = segments.reduce((s, t) => s + (t.end_ms - t.start_ms), 0) / 1000;
  const rtf = audioSec > 0 && genSec > 0 ? genSec / audioSec : 0;

  log(`Batch complete: ${generated} generated, ${skipped} skipped, ${errors} errors`);
  log(`Generated in ${genSec.toFixed(1)}s | RTF ${rtf.toFixed(3)}`);

  ensureDir(join(taskDir, "tts"));
  writeJson(tts_filepath(taskDir), { segments: ttsSegments });
  await setStage(taskDir, "tts", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: "TTS done",
  });
}
