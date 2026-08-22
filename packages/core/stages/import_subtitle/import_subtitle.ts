import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { writeJson, ensureDir } from "@repo/core/utils/fileOps";
import {
  emitLog,
  nowISO,
  probeDuration,
  video_source_path,
} from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { srtTime } from "@repo/core/utils/utils";

/** 与 asr_fix/asr_fix.json 完全一致的单段字幕结构。 */
export interface ImportedSubtitleSegment {
  id?: number;
  text: string;
  start: number;
  end: number;
  start_fmt?: string;
  end_fmt?: string;
  confidence?: number;
}

/** 字幕文件格式。 */
export type SubtitleFileFormat = "vtt" | "srt";

const HTML_ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&apos;": "'",
  "&nbsp;": " ",
};

/** 解码常见 HTML 实体（含数字实体如 &#39;），用于清理 YouTube 自动字幕。 */
export function decodeHtmlEntities(text: string): string {
  return text.replace(/&(amp|lt|gt|quot|apos|nbsp|#\d{2,4});/g, (m, code: string) => {
    if (code.startsWith("#")) return String.fromCharCode(Number(code.slice(1)));
    return HTML_ENTITIES[`&${code};`] ?? m;
  });
}

/**
 * 清理 cue 文本：去掉 <c>/<i>/<00:00:00.200> 等所有标签、合并空白、解码 HTML 实体。
 * YouTube 自动字幕的单词级时间戳 <00:00:00.200> 也在此被移除。
 */
export function cleanCueText(raw: string): string {
  return decodeHtmlEntities(
    raw
      .replace(/<[^>]+>/g, " ")
      .replace(/\s+/g, " ")
      .trim(),
  );
}

/** 把 hh:mm:ss.mmm / mm:ss.mmm（或 SRT 的逗号毫秒）时间戳转为毫秒。解析失败返回 null。 */
export function tsToMs(raw: string): number | null {
  const m = raw.trim().match(/^(?:(\d+):)?(\d{1,2}):(\d{2})[.,](\d{1,3})$/);
  if (!m) return null;
  const [, h, min, sec, msRaw] = m;
  const ms = Number(msRaw.padEnd(3, "0"));
  return Number(h || 0) * 3_600_000 + Number(min) * 60_000 + Number(sec) * 1_000 + ms;
}

/** 从 cue 时间行取 start/end（毫秒），兼容尾部属性如 "align:start position:0%" 与 SRT 的逗号毫秒。 */
function parseCueTimeLine(line: string): { start: number; end: number } | null {
  const sep = line.indexOf("-->");
  if (sep === -1) return null;
  const start = tsToMs(line.slice(0, sep));
  const end = tsToMs(
    line
      .slice(sep + 3)
      .trim()
      .split(/\s+/)[0],
  );
  if (start == null || end == null || end <= start) return null;
  return { start, end };
}

/** 解析 VTT（兼容 YouTube 自动字幕：BOM、WEBVTT 头、Kind/Language/X-TIMESTAMP-MAP、NOTE/STYLE 块、<c>/<i>/单词级时间戳标签、尾部 align/position 属性）。 */
export function parseVtt(content: string): ImportedSubtitleSegment[] {
  const text = content.replace(/^\uFEFF/, "");
  const blocks = text.split(/\r?\n\r?\n/);
  const segments: ImportedSubtitleSegment[] = [];
  for (const block of blocks) {
    const lines = block.split(/\r?\n/);
    if (!lines.length || !lines[0].trim()) continue;
    const head = lines[0].trim();
    if (/^WEBVTT/i.test(head) || /^NOTE\b/i.test(head) || /^STYLE\b/i.test(head)) continue;
    const timeIdx = lines.findIndex((l) => l.includes("-->"));
    if (timeIdx === -1) continue;
    const times = parseCueTimeLine(lines[timeIdx]);
    if (!times) continue;
    const textContent = cleanCueText(lines.slice(timeIdx + 1).join(" "));
    if (!textContent) continue;
    segments.push({ text: textContent, start: times.start, end: times.end });
  }
  return segments;
}

/** 解析 SRT（序号行 + hh:mm:ss,mmm 时间行 + 多行文本块）。 */
export function parseSrt(content: string): ImportedSubtitleSegment[] {
  const text = content.replace(/^\uFEFF/, "");
  const blocks = text.split(/\r?\n\r?\n/);
  const segments: ImportedSubtitleSegment[] = [];
  for (const block of blocks) {
    const lines = block
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean);
    if (lines.length < 2) continue;
    const timeIdx = lines.findIndex((l) => l.includes("-->"));
    if (timeIdx === -1) continue;
    const times = parseCueTimeLine(lines[timeIdx]);
    if (!times) continue;
    const textContent = cleanCueText(lines.slice(timeIdx + 1).join(" "));
    if (!textContent) continue;
    segments.push({ text: textContent, start: times.start, end: times.end });
  }
  return segments;
}

/** 自动识别字幕格式：以 WEBVTT 头开头（允许 BOM）则为 VTT，否则按 SRT 解析。 */
export function detectSubtitleFormat(content: string): SubtitleFileFormat {
  return /^WEBVTT/i.test(content.replace(/^\uFEFF/, "").trim()) ? "vtt" : "srt";
}

/** 与 asr_fix.ts 保持一致的段间 padding，避免字幕贴边。 */
function padSegments(
  segments: ImportedSubtitleSegment[],
  startPad = 100,
  endPad = 300,
): ImportedSubtitleSegment[] {
  if (!segments.length) return segments;
  const minGap = 50;

  const startPadAt = (idx: number): number => {
    const origStart = segments[idx].start;
    if (idx === 0) return Math.max(0, origStart - startPad);
    const prevEnd = segments[idx - 1].end;
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
    const origEnd = segments[idx].end;
    if (idx === segments.length - 1) return origEnd + endPad;
    const nextStart = segments[idx + 1].start;
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
    return { ...s, start: Math.max(0, newStart), end: newEnd };
  });
}

/**
 * import_subtitle 阶段：读取外部 VTT/SRT 字幕文件，转换为与 asr_fix/asr_fix.json
 * 完全一致的标准字幕 JSON（含 audio_info.duration），供下游 translate/split_audio/
 * tts/merge_audio/merge_video 无感知复用。
 *
 * 字幕文件路径优先级：stages.import_subtitle.file > task.subtitleFile。
 */
export async function stageImportSubtitle(ctx: TaskCtx): Promise<void> {
  const taskDir = ctx.task.task_dir;
  const importCfg = ctx.input?.stages?.import_subtitle;
  const subtitleFile = importCfg?.file ?? ctx.input?.task?.subtitleFile;
  if (!subtitleFile) {
    throw new Error(
      '[Import Subtitle] 未指定字幕文件：请设置 task.subtitleFile 或 stages.import_subtitle.file（需配合 task.subtitleSource: "file"）',
    );
  }
  if (!existsSync(subtitleFile)) {
    throw new Error(`[Import Subtitle] 字幕文件不存在: ${subtitleFile}`);
  }

  emitLog(taskDir, `[Import Subtitle] Reading subtitle file: ${subtitleFile}`);
  const content = readFileSync(subtitleFile, "utf-8");

  const format = detectSubtitleFormat(content);
  let segments: ImportedSubtitleSegment[] =
    format === "vtt" ? parseVtt(content) : parseSrt(content);
  if (!segments.length) {
    throw new Error(
      `[Import Subtitle] 字幕文件解析结果为空（格式或时间戳不可识别）: ${subtitleFile}`,
    );
  }
  emitLog(
    taskDir,
    `[Import Subtitle] Detected ${format.toUpperCase()}, parsed ${segments.length} cues`,
  );

  // audio_info.duration（单位 ms）：优先用视频源 probe 时长（probeDuration 返回秒，需 ×1000），失败时用最后一段 end 兜底
  let duration = 0;
  try {
    const videoPath = video_source_path(ctx);
    if (existsSync(videoPath)) duration = Math.round(probeDuration(videoPath) * 1000);
  } catch (e) {
    emitLog(taskDir, `[Import Subtitle] probe duration failed, fallback to last cue end: ${e}`);
  }
  if (!duration) duration = segments[segments.length - 1].end;

  const segmentPad = importCfg?.segmentPad ?? true;
  if (segmentPad) segments = padSegments(segments);

  segments = segments.map((s, idx) => ({
    ...s,
    id: idx + 1,
    confidence: 1,
    start_fmt: srtTime(s.start),
    end_fmt: srtTime(s.end),
  }));
  const resultText = segments.map((s) => s.text).join(" ");

  const asrFixDir = join(taskDir, "asr_fix");
  const srtFile = join(asrFixDir, "asr_fix.json");
  ensureDir(asrFixDir, ctx);
  writeJson(
    srtFile,
    {
      audio_info: { duration },
      result: { text: resultText, segments },
      _llm_fixed: false,
      _source: "import_subtitle",
    },
    ctx,
  );

  emitLog(
    taskDir,
    `[Import Subtitle] Written ${segments.length} segs to asr_fix/asr_fix.json (duration=${duration}ms)`,
  );

  await setStage(taskDir, "import_subtitle", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: `Imported ${segments.length} segs from ${format.toUpperCase()}`,
  });
}
