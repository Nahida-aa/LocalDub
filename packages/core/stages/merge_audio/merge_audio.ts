import { readJson, writeFile } from "@repo/core/utils/fileOps";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { existsSync } from "node:fs";
import { join } from "node:path";
import {
  readTaskLanguages,
  ffmpeg,
  nowISO,
  probeSampleRate,
  probeDuration,
  split_audio_path,
  timings_filepath,
  read_split_audio,
  read_split_audio_timings,
} from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage, setTask } from "@repo/core/context/context.ts";
import { SplitAudioTiming } from "../06_split_audio/out";
import { Timing } from "./types";

/**
 * `merge_audio.ts` 负责把 TTS 生成的各段音频合并成一条完整配音轨，同时做 timing 微调使配音与原视频时间线对齐。
 * **流程：**
 1. 读 `split_audio/timings.json` 得到每段的视频意图起止时间，构建 TTS wav 路径
 2. 逐段处理：
    - 去尾静音（`areverse + silenceremove` 反向去尾，不伤内部停顿）
    - 计算 **advance**（从前间隙借时间，让段略微提前开始）和 **delay**（从后间隙借时间）
    - 若 TTS 时长 ≤ 可用槽位 → 直接复制；超了则 `rubberband` 加速（上限 `maxSpeed` 1.35x）
    - **drift** 累加传到下一段，防止误差累积偏移
 3. 各段之间如果有空隙，插入静音填充
 4. 用 ffmpeg concat 合并所有片段为 `merge_audio/audio_dubbing.wav`
 5. 输出 `merge_audio/timings.json` 含每段实际起止时间、拉伸比、drift 等

 核心设计点：drift 传播 + advance/delay 借间隙时间，让配音节奏自然而不破坏整体同步。
 */
export async function stageMergeAudio(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  const mergeAudioDir = join(taskDir, "merge_audio");
  const ttsDir = join(taskDir, "tts", "wavs");
  const stretchedDir = join(mergeAudioDir, "stretched");
  const silenceDir = join(mergeAudioDir, "silences");

  ensureDir(stretchedDir);
  ensureDir(silenceDir);
  ensureDir(mergeAudioDir);

  const dubbingFile = join(mergeAudioDir, "audio_dubbing.wav");

  const data = await read_split_audio_timings(ctx);
  const segments = data.segments;
  const ttsFiles = segments.map((_: any, i: number) =>
    join(ttsDir, `${String(i + 1).padStart(4, "0")}.wav`),
  );

  for (const f of ttsFiles) {
    if (!existsSync(f)) throw new Error(`Missing TTS segment: ${f}`);
  }

  const sampleRate = probeSampleRate(ttsFiles[0]);

  const segmentInputs: string[] = [];
  let lastEndMs = 0;
  let drift = 0;

  const maxSpeed = ctx.input?.stages?.merge_audio?.maxSpeed;
  const maxAdvanceMs = ctx.input?.stages?.merge_audio?.maxAdvanceMs;
  const maxDelayMs = ctx.input?.stages?.merge_audio?.maxDelayMs;
  const newTranslation: Timing[] = [];
  for (const [i, item] of segments.entries()) {
    // const segment = translation[i];
    const ttsFile = ttsFiles[i];
    const idx = String(i + 1).padStart(4, "0");
    const stretchedFile = join(stretchedDir, `${idx}.wav`);

    // Probe original TTS duration
    const ttsSec = probeDuration(ttsFile);

    // Trim trailing silence only (areverse so internal pauses aren't mistaken for tail)
    const trimmedFile = join(stretchedDir, `${idx}_trimmed.wav`);
    ffmpeg([
      "-i",
      ttsFile,
      "-af",
      "areverse,silenceremove=start_periods=1:start_threshold=-50dB:start_duration=0.05,areverse",
      trimmedFile,
    ]);

    const trimmedSec = probeDuration(trimmedFile);

    // Determine advance — conservative for segments that already fit
    const originalSlotBaseSec = (item.end_ms - item.start_ms) / 1000;
    let advanceMs = 0;
    if (trimmedSec <= originalSlotBaseSec) {
      const surplusNoAdvanceSec = drift + (originalSlotBaseSec - trimmedSec);
      if (surplusNoAdvanceSec < 0.5) {
        advanceMs = Math.min(
          Math.round((0.5 - surplusNoAdvanceSec) * 1000),
          Math.round(maxAdvanceMs * 0.2),
        );
      }
    } else {
      advanceMs = Math.min(maxAdvanceMs, Math.max(0, Math.round(drift * 1000)));
    }

    const realStartMs = Math.max(item.start_ms - advanceMs, lastEndMs, 0);
    advanceMs = Math.max(0, item.start_ms - realStartMs);
    const effectiveDrift = drift - advanceMs / 1000;

    // Determine delay — borrow time from the next segment's gap
    const nextStartMs = i < segments.length - 1 ? segments[i + 1].start_ms : item.end_ms;
    const gapMs = Math.max(0, nextStartMs - item.end_ms);
    const delayMs = Math.min(gapMs, maxDelayMs);

    if (realStartMs > lastEndMs) {
      const gapSec = (realStartMs - lastEndMs) / 1000;
      const silenceFile = join(silenceDir, `silence_${i}.wav`);
      ffmpeg([
        "-f",
        "lavfi",
        "-i",
        `anullsrc=r=${sampleRate}:cl=mono`,
        "-t",
        String(gapSec),
        silenceFile,
      ]);
      segmentInputs.push(silenceFile);
    }

    const originalSlotSec = (item.end_ms + delayMs - realStartMs) / 1000;
    // floor at 50ms so speed calc never goes negative
    const slotSec = Math.max(0.05, originalSlotSec + effectiveDrift);

    let stretchedSec: number;
    let newDrift: number;
    let speed = 1.0;
    if (trimmedSec <= originalSlotSec) {
      stretchedSec = trimmedSec;
      ffmpeg(["-i", trimmedFile, "-c", "copy", stretchedFile]);
    } else if (trimmedSec <= slotSec) {
      stretchedSec = trimmedSec;
      ffmpeg(["-i", trimmedFile, "-c", "copy", stretchedFile]);
    } else {
      speed = Math.min(maxSpeed, trimmedSec / slotSec);
      stretchedSec = trimmedSec / speed;
      ffmpeg([
        "-i",
        trimmedFile,
        "-filter:a",
        `rubberband=tempo=${speed.toFixed(4)}`,
        stretchedFile,
      ]);
    }
    newDrift = originalSlotSec - stretchedSec;
    if (newDrift > maxAdvanceMs / 1000) newDrift = maxAdvanceMs / 1000;

    drift = newDrift;
    segmentInputs.push(stretchedFile);

    const realEndMs = Math.floor(realStartMs + stretchedSec * 1000);

    if (realEndMs <= realStartMs) {
      throw new Error(
        `[merge_audio] #${i + 1} (${item.text?.slice(0, 30) || "?"}) 生成了零时长段: ` +
          `tts_file=${ttsFile}, ` +
          `item.start=${item.start_ms}ms, item.end=${item.end_ms}ms (slot=${(item.end_ms - item.start_ms).toFixed(0)}ms), ` +
          `tts_duration=${(ttsSec * 1000).toFixed(0)}ms, trimmed=${(trimmedSec * 1000).toFixed(0)}ms, ` +
          `advance=${advanceMs}ms, delay=${delayMs}ms, ` +
          `stretched=${(stretchedSec * 1000).toFixed(0)}ms, drift=${drift.toFixed(3)}s\n` +
          `可能原因: TTS 生成了空音频 (检查 tts/tts.json 该段 status) 或原始时间槽为 0`,
      );
    }

    lastEndMs = realEndMs;
    const segment: Timing = {
      ...item,
      original_duration_ms: item.end_ms - item.start_ms,
      drift_ms: Math.round(drift * 1000),
      advance_ms: advanceMs,
      delay_ms: delayMs,
      actual_start: Math.floor(realStartMs),
      actual_end: realEndMs,
      tts_duration_ms: Math.round(ttsSec * 1000),
      stretched_duration_ms: Math.round(stretchedSec * 1000),
      stretch_ratio: parseFloat((trimmedSec <= slotSec ? 1.0 : speed).toFixed(4)),
    };
    newTranslation.push(segment);
  }

  if (segmentInputs.length === 0) throw new Error("No audio segments to merge");

  const concatFile = join(mergeAudioDir, "concat_list.txt");
  writeFile(concatFile, segmentInputs.map((f) => `file '${f}'`).join("\n"), ctx);
  // 连接所有配音片段并输出最终配音音频
  ffmpeg([
    "-f",
    "concat", // 使用 concat 分离器
    "-safe",
    "0", // 允许文件路径中的特殊字符
    "-i",
    concatFile, // 输入文件列表（每行 `file 'path'`）
    "-acodec",
    "pcm_s16le", // 输出编码：16-bit 有符号小端 PCM（WAV
    "-ar",
    String(sampleRate), // 采样率，沿用 TTS 的采样率
    "-ac",
    "1", // 单声道
    dubbingFile,
  ]); // 输出到 `merge_audio/audio_dubbing.wav

  writeJson(timings_filepath(taskDir), { translation: newTranslation });
  await setStage(taskDir, "merge_audio", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: "Merged",
  });
}
