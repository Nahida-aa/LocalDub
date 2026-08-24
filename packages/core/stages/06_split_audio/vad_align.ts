import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { cutAudioRange } from "./util";
import { env } from "@repo/config/env";
import { probeDurationMs } from "@repo/core/utils/ffmpeg";
import { SplitAudioSegment, SplitAudioTiming } from "./out";
import { log } from "@repo/util/log";

/**
 * 检测一段已切好 wav 开头被静音削掉的时长 (ms)。
 *
 * 做法: 用 ffmpeg silenceremove 把 >0.1s 且 < -30dB 的起始静音裁掉,
 * 对比原时长减裁后时长。若原时长异常则返回 0。
 */
function detectSpeechStartMs(wavPath: string): number {
  const origMs = probeDurationMs(wavPath);
  if (origMs <= 0) return 0;

  const tmpPath = wavPath.replace(".wav", ".trim.wav");
  const r = spawnSync(
    env.FFMPEG_PATH,
    [
      "-i",
      wavPath,
      "-af",
      "silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1",
      "-y",
      tmpPath,
    ],
    { stdio: ["pipe", "pipe", "pipe"], timeout: 30_000 },
  );

  let removedMs = 0;
  if (r.status === 0) {
    removedMs = origMs - probeDurationMs(tmpPath);
  }
  rmSync(tmpPath, { force: true });
  return Math.max(0, removedMs);
}

/**
 * 同 detectSpeechStartMs, 但直接从源音轨 seek [startMs, endMs) 裁出临时 wav 再测,
 * 用于段音频尚未生成 (或未用 vocals) 时估算开头的静音时长。
 */
function detectSpeechStartMsSeek(
  source: string,
  startMs: number,
  endMs: number,
  workDir: string,
): number {
  const durMs = endMs - startMs;
  if (durMs <= 0) return 0;

  const tmpPath = join(workDir, ".vad_trim.wav");
  const r = spawnSync(
    env.FFMPEG_PATH,
    [
      "-ss",
      String(startMs / 1000),
      "-to",
      String(endMs / 1000),
      "-i",
      source,
      "-vn",
      "-af",
      "silenceremove=start_periods=1:start_threshold=-30dB:start_duration=0.1",
      "-y",
      tmpPath,
    ],
    { stdio: ["pipe", "pipe", "pipe"], timeout: 30_000 },
  );

  let removedMs = 0;
  if (r.status === 0) {
    removedMs = durMs - probeDurationMs(tmpPath);
  }
  rmSync(tmpPath, { force: true });
  return Math.max(0, removedMs);
}

/** 可选 vadAlign: 用静音检测把每段起点前移到真实语音处, 返回是否修正了任何段 */
export function applyVadAlign(opts: {
  segments: SplitAudioSegment[]; // 每段的 split_start/end; 会被改写
  timings: SplitAudioTiming[]; // 意图时序; start_ms 会被改写
  sourceAudio: string;
  totalMs: number;
  vocalsSegmentDir: string;
  hasVocals: boolean;
}): boolean {
  let corrected = false;
  for (let i = 0; i < opts.segments.length; i++) {
    const startMs = opts.segments[i].split_start_ms;
    const endMs = opts.segments[i].split_end_ms;
    if (startMs >= endMs) continue;

    // 有切好的块就测块, 否则直接从源音轨 seek 一小段临时测
    const wavPath = join(opts.vocalsSegmentDir, `${String(i + 1).padStart(4, "0")}.wav`);
    const removedMs = existsSync(wavPath)
      ? detectSpeechStartMs(wavPath)
      : detectSpeechStartMsSeek(
          opts.sourceAudio,
          startMs, // 已是 padSegments 后的 split_start
          Math.min(opts.totalMs, endMs),
          opts.vocalsSegmentDir,
        );
    if (removedMs <= 500) continue; // 开头静音不足 500ms, 不值得修正

    // 起点前移 removedMs-80 (保留 80ms 呼吸余量)
    const cutStartMs = opts.segments[i].split_start_ms;
    const newCutStartMs = cutStartMs + removedMs - 80;
    if (newCutStartMs >= endMs) {
      log(`vadAlign #${i + 1}: would exceed end (${newCutStartMs} >= ${endMs}), truncating`);
      continue;
    }

    log(`vadAlign #${i + 1}: start ${cutStartMs} → ${newCutStartMs} (removed ${removedMs}ms)`);

    // 同步把块重新切 (从新起点到原来的结尾+余量), 并更新两条时序的 start
    if (opts.hasVocals) {
      const newEnd = Math.min(opts.totalMs, endMs + 160);
      if (newEnd > newCutStartMs) {
        cutAudioRange(opts.sourceAudio, newCutStartMs, newEnd, wavPath);
      }
    }

    opts.segments[i].split_start_ms = newCutStartMs;
    opts.timings[i].start_ms = Math.max(0, opts.timings[i].start_ms + removedMs - 80);
    corrected = true;
  }
  return corrected;
}
