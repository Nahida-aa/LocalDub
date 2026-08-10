import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { readJson } from "@repo/core/utils/fileOps";
import { writeJson, ensureDir } from "@repo/util/file_op";
import { emitLog, nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage, setTask } from "@repo/core/context/context.ts";
import { srtTime } from "@repo/core/utils/utils";
import { extract_frame, extract_frames } from "@repo/subtitle-ocr/ffmpeg_util";
import { to } from "@repo/shared/lib/utils/try";
import { SubtitlingSegment } from "@repo/subtitling/types";
import { AsrResult } from "../asr/types";
import { log } from "@repo/util/log";

// Split long ASR segments by punctuation using word-level timestamps
const SPLIT_PAT = /[，,。！？.!?]/;
const MIN_SUB_DUR = 800;

// Find word indices where seg.text contains spaces (e.g. "陆 陆直循")
function findSpaceSplits(text: string, words: { word: string }[]): number[] {
  const chars = [...text];
  const splits: number[] = [];
  let wordIdx = 0;
  let wordPos = 0;
  for (let i = 0; i < chars.length && wordIdx < words.length; i++) {
    if (chars[i] === " ") {
      if (wordIdx > 0) splits.push(wordIdx - 1);
      continue;
    }
    wordPos++;
    if (wordPos >= words[wordIdx].word.length) {
      wordIdx++;
      wordPos = 0;
    }
  }
  return splits;
}

function splitAsrByWords(
  segs: {
    text: string;
    start: number;
    end: number;
    words?: { word: string; start: number; end: number; probability: number }[];
  }[],
): SubtitlingSegment[] {
  return segs.flatMap((seg) => {
    const ws = seg.words;
    if (!ws || ws.length < 2) {
      return [{ text: seg.text, start_ms: seg.start, end_ms: seg.end }];
    }
    const spaceSplits = findSpaceSplits(seg.text, ws);
    const punctSplits = (() => {
      const punct: number[] = [];
      for (let i = 0; i < ws.length; i++) {
        if (SPLIT_PAT.test(ws[i].word)) punct.push(i);
      }
      return punct;
    })();
    const hasSpaceSplit = spaceSplits.length > 0;
    const splitIdx: number[] = [...spaceSplits, ...punctSplits]
      .sort((a, b) => a - b)
      .filter((v, i, a) => a.indexOf(v) === i);
    if (splitIdx.length <= 1 && !hasSpaceSplit) {
      return [{ text: seg.text, start_ms: seg.start, end_ms: seg.end }];
    }
    // Filter split points: keep if remaining segment (after the split) >= MIN_SUB_DUR
    const useIdx: number[] = [];
    const totalEnd = ws[ws.length - 1].end;
    for (let i = 0; i < splitIdx.length - 1; i++) {
      const endMs = ws[splitIdx[i]].end;
      if (totalEnd - endMs >= MIN_SUB_DUR) {
        useIdx.push(splitIdx[i]);
      }
    }
    useIdx.push(splitIdx[splitIdx.length - 1]);
    if (useIdx.length <= 1 && !hasSpaceSplit) {
      return [{ text: seg.text, start_ms: seg.start, end_ms: seg.end }];
    }
    const subSegs: SubtitlingSegment[] = [];
    let prevIdx = 0;
    for (let i = 0; i < useIdx.length; i++) {
      const endIdx = useIdx[i];
      subSegs.push({
        text: ws
          .slice(prevIdx, endIdx + 1)
          .map((w) => w.word)
          .join(""),
        start_ms: ws[prevIdx].start,
        end_ms: ws[endIdx].end,
      });
      prevIdx = endIdx + 1;
    }
    if (prevIdx < ws.length) {
      subSegs.push({
        text: ws
          .slice(prevIdx)
          .map((w) => w.word)
          .join(""),
        start_ms: ws[prevIdx].start,
        end_ms: totalEnd,
      });
    }
    return subSegs;
  });
}

export async function stageAsrOcrPre(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;

  setStage(taskDir, "asr_ocr_pre", {
    last_message: "Splitting ASR segments by punctuation...",
    progress: 0,
  });

  const videoPath = video_source_path(ctx);
  if (!existsSync(videoPath)) {
    console.error(`[asr_ocr_pre] Video not found: ${videoPath}`);
    throw new Error(`Video not found: ${videoPath}`);
  }

  const asrFile = join(taskDir, "asr", "asr.json");
  if (!existsSync(asrFile)) {
    console.error(`[asr_ocr_pre] asr.json not found: ${asrFile}`);
    throw new Error(`asr.json not found: ${asrFile}`);
  }

  const asrData = await readJson<AsrResult>(asrFile);
  const asrSegsRaw: {
    text: string;
    start: number;
    end: number;
    words?: { word: string; start: number; end: number; probability: number }[];
  }[] = (asrData.result?.segments ?? []).map((s) => ({
    text: s.text,
    start: Math.round(s.start),
    end: Math.round(s.end),
    words: s.words,
  }));

  if (!asrSegsRaw.length) throw new Error("No ASR segments found");

  // Step 1: Split ASR segments by punctuation
  log(`${asrSegsRaw.length} Split ASR segments by punctuation`);
  const asrSegs = splitAsrByWords(asrSegsRaw);

  const preDir = join(taskDir, "asr_ocr_pre");
  ensureDir(preDir);

  // Write asr_split.json
  writeJson(join(preDir, "asr_split.json"), {
    original_segments_count: asrSegsRaw.length,
    segments_count: asrSegs.length,
    result: {
      text: asrSegs.map((s) => s.text).join(" "),
      segments: asrSegs,
    },
  });

  log(`${asrSegsRaw.length} ASR segs → ${asrSegs.length} split segs`);

  // Step 2: Generate frame timestamps (end2fps strategy)
  await setStage(taskDir, "asr_ocr_pre", {
    last_message: `Extracting ${asrSegs.length} split segments frames...`,
    progress: 10,
  });

  const allTimestamps = new Set<number>();
  for (let i = 0; i < asrSegs.length; i++) {
    const seg = asrSegs[i];
    if (i === 0) {
      let fwd = Math.round(seg.start_ms);
      let bwd = Math.round(seg.end_ms);
      while (fwd <= bwd) {
        allTimestamps.add(fwd);
        if (fwd !== bwd) allTimestamps.add(bwd);
        fwd += 100;
        bwd -= 100;
      }
    } else {
      for (let t = Math.round(seg.end_ms); t >= seg.start_ms; t -= 500) {
        allTimestamps.add(Math.round(t));
      }
    }
  }
  const sortedTs = [...allTimestamps].sort((a, b) => a - b);

  emitLog(
    taskDir,
    `[asr_ocr_pre] ${asrSegs.length} split segs → ${sortedTs.length} frame positions`,
  );

  // Step 3: Extract frames
  const frameDir = join(taskDir, "asr_ocr_pre", "frames");
  ensureDir(frameDir);

  const extractCount = extract_frames(sortedTs, videoPath, frameDir);

  if (!extractCount) {
    throw new Error("No frames extracted");
  }

  emitLog(taskDir, `[asr_ocr_pre] ${extractCount} frames extracted to ${frameDir}`);

  await setStage(taskDir, "asr_ocr_pre", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
  });
}
