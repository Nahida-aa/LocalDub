/**
 * split_audio: 按字幕/翻译时间轴把音频切成逐段 wav, 供 tts 逐段合成。
 *
 * 输入:
 * - 字幕文件 (asr_fix / sf_ocr / asr_ocr 产物, 见 subtitleFilePath) — 权威时间轴
 * - 翻译文件 translate.[dstLang].json (若 translate.enabled) — 合成目标文本
 * - 源音频: 有分离人声 (vocals) 用干净人声, 否则用原视频音轨
 *
 * 输出 (写入 taskDir/split_audio/):
 * - split_audio.json  → segments: SplitAudioSegment[] 经过 padSegments 补齐边界的时序,
 *                       供 tts 逐段读文本合成, 也是切块的真实时间
 * - timings.json      → segments: SplitAudioTiming[] 未 padding 的「意图时序」,
 *                       供 merge_audio/merge_video 定位最终落点
 * - vocals/0001.wav ... 按段切出的音频 (仅 dub 模式下有 vocals 时)
 *
 * 可选 vadAlign: 用 ffmpeg silenceremove 检测每段开头的静音并前移切割起点,
 * 修正 segments 前后静音导致的偏移。
 */
import { readJson } from "@repo/core/utils/fileOps";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { existsSync, readdirSync, statSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import {
  translationFilePath,
  nowISO,
  emitLog,
  subtitleFilePath,
  split_audio_path,
  video_source_path,
  vocalsPath,
  readTranslationResult,
  split_audio_timings_path,
} from "@repo/core/stages/utils/utils.ts";
import { env } from "@repo/config/env";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import {
  SplitAudioResult,
  SplitAudioSegment,
  SplitAudioTiming,
  SplitAudioTimingResult,
} from "./out";
import { resolveLanguage } from "../05_translate/utils";
import { SrtJson } from "@repo/subtitle/types";
import { log } from "@repo/util/log";
import { applyVadAlign } from "./vad_align";
import { cutAudioRange } from "./util";

/** 用 ffprobe 取媒体总时长 (毫秒) */
function probeDuration(file: string): number {
  const r = spawnSync(
    "ffprobe",
    ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", file],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  return Math.floor(parseFloat(r.stdout.toString().trim()) * 1000) || 0;
}

/**
 * 给各段时间轴加前后 padding (默认前 100ms / 后 300ms), 避免切块时把语音截断。
 *
 * 规则:
 * - 每段独立计算 start/end 的 padding 量, 相邻段之间的空白 (gap) 越充足则越接满默认值;
 * - gap 不足时按比例分摊 start/end, 保证不越过相邻段;
 * - minGap=50ms 之下的缝直接取中点, 避免与相邻段重叠。
 * 返回新数组, 不修改原数组。
 */
function padSegments(
  segments: SplitAudioTiming[],
  startPad = 100,
  endPad = 300,
): SplitAudioSegment[] {
  if (!segments.length) return [];
  const minGap = 50;

  const startPadAt = (idx: number): number => {
    const origStart = segments[idx].start_ms;
    if (idx === 0) return Math.max(0, origStart - startPad);
    const prevEnd = segments[idx - 1].end_ms;
    const gap = origStart - prevEnd;
    const total = startPad + endPad;
    if (gap >= total + minGap) return origStart - startPad;
    if (gap > minGap) {
      const share = ((gap - minGap) * startPad) / total;
      return origStart - share;
    }
    return prevEnd + gap / 2;
  };

  const endPadAt = (idx: number): number => {
    const origEnd = segments[idx].end_ms;
    if (idx === segments.length - 1) {
      return origEnd + endPad;
    }
    const nextStart = segments[idx + 1].start_ms;
    const gap = nextStart - origEnd;
    const total = startPad + endPad;
    if (gap >= total + minGap) return origEnd + endPad;
    if (gap > minGap) {
      const share = ((gap - minGap) * endPad) / total;
      return origEnd + share;
    }
    return origEnd + gap / 2;
  };

  return segments.map((s, idx) => {
    const newStart = startPadAt(idx);
    const newEnd = endPadAt(idx);
    return { ...s, split_start_ms: Math.max(0, newStart), split_end_ms: newEnd };
  });
}

export async function stageSplitAudio(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;

  // 权威字幕文件 (asr_fix/sf_ocr/asr_ocr 按 subtitleSource 决定)；源音频可被 split_audio.sourceFilePath 覆盖
  const srtFilePath = subtitleFilePath(ctx);
  const sourceFilePath = ctx.input?.stages?.split_audio?.sourceFilePath ?? video_source_path(ctx);
  const { srcLang, targetLang } = resolveLanguage(ctx);

  const splitAudioDir = join(taskDir, "split_audio");
  const translationFile = translationFilePath(taskDir, targetLang);
  const splitAudioPath = split_audio_path(taskDir); // split_audio.json (padding 后时序)
  const timingsFile = split_audio_timings_path(taskDir); // timings.json (意图时序)
  const vocalsSegmentDir = join(splitAudioDir, "vocals");

  if (!existsSync(srtFilePath)) throw new Error(`subtitle file not found: ${srtFilePath}`);
  // 优先切分离出的干净人声 (vocals), 没有就切原音轨
  const vocalsFilePath = ctx.input?.stages?.split_audio?.vocalsFilePath ?? vocalsPath(taskDir);
  const hasVocals = vocalsFilePath ? existsSync(vocalsFilePath) : false;
  const sourceAudio = hasVocals ? vocalsFilePath! : sourceFilePath;

  // 从字幕文件拿权威时间轴 (毫秒)
  const srtData = await readJson<SrtJson>(srtFilePath);
  const srtSegments = srtData.result?.segments;
  if (!srtSegments?.length) throw new Error(`${srtFilePath} has no segments`);

  // 拼 timings: 有翻译就取 translate.[dstLang].json 的 dst 文本, 否则退回用原字幕文本
  const translateEnabled = ctx.input.stages.translate.enabled;
  const timings: SplitAudioTiming[] = await (async () => {
    if (translateEnabled) {
      const transData = await readTranslationResult(ctx);
      const translation = transData.segments;
      if (!translation?.length) throw new Error("translation.json has no segments");
      return translation.map((seg, i) => ({
        seg_idx: i + 1,
        text: translation[i].text,
        dst: translation[i].dst,
        src_lang: translation[i].src_lang,
        dst_lang: translation[i].dst_lang,
        start_ms: translation[i].start_ms,
        end_ms: translation[i].end_ms,
        speaker: translation[i].speaker,
      }));
    } else {
      return srtSegments.map((seg, i) => ({
        seg_idx: i + 1,
        text: seg.text,
        dst: seg.text, // 未翻译时 dst 直接用原文, 保证 tts 有文本可读
        src_lang: srcLang,
        dst_lang: srcLang,
        start_ms: seg.start_ms,
        end_ms: seg.end_ms,
      }));
    }
  })();

  // timings 应用 padding, 得到真实的切块时序 (写进 split_audio.json)
  const splitAudioSegments = padSegments(timings);

  // 源音频总时长, 用于上界截断
  let totalMs = probeDuration(sourceAudio);

  ensureDir(vocalsSegmentDir);
  ensureDir(splitAudioDir);

  // 切块 (仅 dub 模式有 vocals 时执行; subtitle 模式跳过, 只产出时序文件)
  if (hasVocals) {
    // 若翻译文件比已切出的块更新 (重跑翻译), 清空旧块重新切
    const anySeg = readdirSync(vocalsSegmentDir).find((f) => f.endsWith(".wav"));
    if (
      anySeg &&
      existsSync(translationFile) &&
      statSync(translationFile).mtimeMs > statSync(join(vocalsSegmentDir, anySeg)).mtimeMs
    ) {
      for (const f of readdirSync(vocalsSegmentDir)) rmSync(join(vocalsSegmentDir, f));
    }

    for (let i = 0; i < splitAudioSegments.length; i++) {
      const idx = String(i + 1).padStart(4, "0");
      const outPath = join(vocalsSegmentDir, `${idx}.wav`);
      if (existsSync(outPath)) continue; // 已切过就跳过 (支持断点续跑)

      const startMs = splitAudioSegments[i].split_start_ms;
      const endMs = splitAudioSegments[i].split_end_ms;
      if (startMs >= endMs) {
        // 无效段: 写 44 字节空 wav 头占位, 后续 tts 直接跳过
        writeFileSync(outPath, Buffer.alloc(44));
        log(`#${i + 1} invalid (${startMs} >= ${endMs}), empty wav`);
        continue;
      }

      // 前后各留 80ms/160ms 余量, 收在源音频范围内
      const start = Math.max(0, startMs - 80);
      const end = Math.min(totalMs, endMs + 160);
      if (end <= start) {
        writeFileSync(outPath, Buffer.alloc(44));
        continue;
      }

      cutAudioRange(sourceAudio, start, end, outPath);
    }
  }
  const splitAudioResultMeta = {
    src_lang: srcLang,
    target_lang: translateEnabled ? targetLang : srcLang,
  };
  const splitAudioResult: SplitAudioResult = {
    segments: splitAudioSegments,
    meta: splitAudioResultMeta,
  };
  // 切块后把 padding 时序写盘 (tts 逐段读它)
  writeJson(splitAudioPath, splitAudioResult);

  // ---- VAD alignment (可选): 用静音检测把每段起点前移到真实语音处 ----
  const splitArgs = ctx.input.stages.split_audio;

  if (splitArgs.vadAlign) {
    const corrected = applyVadAlign({
      segments: splitAudioSegments,
      timings,
      sourceAudio,
      totalMs,
      vocalsSegmentDir,
      hasVocals,
    });
    // applyVadAlign 会原地改写 splitAudioSegments, 修正过才重新写盘
    if (corrected) writeJson(splitAudioPath, splitAudioResult);
  }
  const splitAudioTimingResult: SplitAudioTimingResult = {
    segments: timings,
  };
  // 意图时序始终写盘 (无 vadAlign 或无修正时也恢复最新意图起点)
  writeJson(timingsFile, splitAudioTimingResult);

  setStage(taskDir, "split_audio", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: "Split",
  });
}
