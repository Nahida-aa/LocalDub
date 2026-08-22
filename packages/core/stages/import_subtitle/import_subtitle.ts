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

/** 按词切分文本（单词/数字/撇号），用于重叠检测；忽略大小写。 */
function tokenizeWords(text: string): string[] {
  return text.match(/[A-Za-zÀ-ÖØ-öø-ÿ0-9']+/g) ?? [];
}

function sameWordArr(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].toLowerCase() !== b[i].toLowerCase()) return false;
  }
  return true;
}

/** 返回文本中第 wordIndex 个词（0 基）的起始字符下标。 */
function findWordStart(text: string, wordIndex: number): number {
  const re = /\S+/g;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text))) {
    if (i === wordIndex) return m.index;
    i++;
  }
  return text.length;
}

/** 计算 a 的尾部与 b 的头部的最长重叠（按词计）。 */
function longestPrefixSuffixOverlap(a: string, b: string): { overlapWords: number } {
  const aw = tokenizeWords(a);
  const bw = tokenizeWords(b);
  const max = Math.min(aw.length, bw.length);
  for (let k = max; k >= 1; k--) {
    if (sameWordArr(aw.slice(aw.length - k), bw.slice(0, k))) {
      return { overlapWords: k };
    }
  }
  return { overlapWords: 0 };
}

/** 合并两段文本：取较长覆盖 + 去重重叠尾部/头部，中间以单个空格衔接。 */
function mergeTwoTexts(a: string, b: string): string {
  const aw = tokenizeWords(a);
  const bw = tokenizeWords(b);
  // a 是 b 的前缀 → 取 b（渐进窗口的新内容在 b）
  if (bw.length >= aw.length && sameWordArr(aw, bw.slice(0, aw.length))) return b;
  // b 是 a 的前缀 → 取 a（b 是回声预告，不含新内容）
  if (aw.length >= bw.length && sameWordArr(bw, aw.slice(0, bw.length))) return a;
  // 一般重叠：a 尾 b 头共享 overlapWords 个词，b 去掉重复部分后拼到 a 后面
  const { overlapWords } = longestPrefixSuffixOverlap(a, b);
  if (overlapWords > 0) {
    return a.trimEnd() + " " + b.slice(findWordStart(b, overlapWords)).trim();
  }
  return a.trimEnd() + " " + b.trim();
}

/**
 * 合并相邻重叠字幕段（YouTube 自动字幕的渐进式滑动窗口输出）。
 *
 * 特征：每条 cue 重复上一条的尾部 + 新增内容，偶数条常为 10ms 的"回声预告"。
 * 合并条件：两段时间上连续（gap ≤ maxGapMs）且文本重叠词数达标；
 * 极短段（回声）只需 ≥1 词重叠即吸收。
 * 对无重叠的干净字幕（普通 SRT）完全幂等，不产生任何改动。
 */
export function mergeOverlapSegments(
  segments: ImportedSubtitleSegment[],
  opts: { minOverlapWords?: number; maxGapMs?: number } = {},
): { segments: ImportedSubtitleSegment[]; mergedCount: number } {
  const { minOverlapWords = 2, maxGapMs = 800 } = opts;
  if (!segments.length) return { segments, mergedCount: 0 };

  const out: ImportedSubtitleSegment[] = [];
  let cur: ImportedSubtitleSegment = { ...segments[0] };
  let mergedCount = 0;
  for (let i = 1; i < segments.length; i++) {
    const seg = segments[i];
    const isEcho = seg.end - seg.start < 150;
    const { overlapWords } = longestPrefixSuffixOverlap(cur.text, seg.text);
    const gapMs = seg.start - cur.end; // 可为负（时间重叠）
    const overlapOk = overlapWords >= minOverlapWords || (isEcho && overlapWords >= 1);
    if (gapMs <= maxGapMs && overlapOk) {
      cur.end = seg.end;
      cur.text = mergeTwoTexts(cur.text, seg.text);
      mergedCount++;
      continue;
    }
    out.push(cur);
    cur = { ...seg };
  }
  out.push(cur);
  return { segments: out, mergedCount };
}

/**
 * 把单段内的长文本按句子边界（. ! ? …，后接空白+大写或段尾）拆成独立句子，
 * 并按字符占比对句子时间做线性插值。缩写（Mr./e.g./U.S.）后接小写时不拆分。
 */
export function splitBySentences(segments: ImportedSubtitleSegment[]): ImportedSubtitleSegment[] {
  const out: ImportedSubtitleSegment[] = [];
  for (const seg of segments) {
    const sentences = splitTextIntoSentences(seg.text);
    if (sentences.length <= 1) {
      out.push(seg);
      continue;
    }
    const totalChars = seg.text.replace(/\s/g, "").length || 1;
    const span = seg.end - seg.start;
    let cursor = seg.start;
    sentences.forEach((s, i) => {
      const chars = s.replace(/\s/g, "").length;
      const isLast = i === sentences.length - 1;
      const start = cursor;
      const end = isLast ? seg.end : Math.round(cursor + (chars / totalChars) * span);
      out.push({ ...seg, text: s, start, end });
      cursor = end;
    });
  }
  return out;
}

/**
 * TTS 配音段细分：句子词数 > maxWords 时切分，切点优先落在逗号处；
 * 无逗号（或无更多逗号）时从后往前按词边界切，保证每段 ≤ maxWords 词。
 * 时间按字符占比线性插值。
 */
export function splitForTTS(
  segments: ImportedSubtitleSegment[],
  maxWords = 10,
): ImportedSubtitleSegment[] {
  const out: ImportedSubtitleSegment[] = [];
  for (const seg of segments) {
    const wordCount = seg.text.split(/\s+/).filter(Boolean).length;
    if (wordCount <= maxWords) {
      out.push(seg);
      continue;
    }
    const parts = segmentSubParts(seg.text, maxWords);
    if (parts.length <= 1) {
      out.push(seg);
      continue;
    }
    const totalChars = seg.text.replace(/\s/g, "").length || 1;
    const span = seg.end - seg.start;
    let cursor = seg.start;
    parts.forEach((p, i) => {
      const chars = p.replace(/\s/g, "").length;
      const isLast = i === parts.length - 1;
      const start = cursor;
      const end = isLast ? seg.end : Math.round(cursor + (chars / totalChars) * span);
      out.push({ ...seg, text: p, start, end });
      cursor = end;
    });
  }
  return out;
}

/** 把一段文本切成 ≤ maxWords 词的小段：优先逗号处切，剩余无逗号部分从后往前切。 */
function segmentSubParts(text: string, maxWords: number): string[] {
  const commaParts = text.split(/(?<=,)\s+/);
  const parts: string[] = [];
  for (const p of commaParts) {
    const tokens = p.split(/\s+/).filter(Boolean);
    if (tokens.length <= maxWords) {
      parts.push(p);
      continue;
    }
    // 从后往前切：先切出尾部 maxWords 词，剩余继续，切点落在词边界
    let end = tokens.length;
    const sub: string[] = [];
    while (end > maxWords) {
      const start = end - maxWords;
      sub.unshift(tokens.slice(start, end).join(" "));
      end = start;
    }
    if (end > 0) sub.unshift(tokens.slice(0, end).join(" "));
    parts.push(...sub);
  }
  return parts;
}

/** 常见英文缩写，句点后接大写（如 "vs. Zombies"）时不视为句界。 */
const ABBREVIATIONS = new Set([
  "vs",
  "mr",
  "mrs",
  "ms",
  "dr",
  "st",
  "prof",
  "rev",
  "sr",
  "jr",
  "etc",
  "eg",
  "ie",
  "no",
  "us",
  "uk",
  "am",
  "pm",
  "inc",
  "ltd",
  "dept",
  "gov",
  "univ",
  "gen",
  "col",
  "capt",
  "sgt",
  "lt",
  "sen",
  "rep",
  "fig",
  "al",
  "approx",
  "est",
  "min",
  "max",
  "avg",
  "sec",
  "hr",
  "oz",
  "lb",
  "ft",
  "in",
  "cm",
  "mm",
  "km",
]);

function splitTextIntoSentences(text: string): string[] {
  const out: string[] = [];
  const re = /[.!?…]+["')\]]*/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const end = m.index + m[0].length;
    const rest = text.slice(end);
    // 缩写保护：句点前一个词是缩写或单字母（如 "U.S."、"vs."），不切分
    const prevWord = text.slice(0, m.index).match(/([A-Za-zÀ-ÖØ-öø-ÿ0-9]+)[.!?…]?$/)?.[1];
    if (prevWord != null && (prevWord.length <= 1 || ABBREVIATIONS.has(prevWord.toLowerCase()))) {
      continue;
    }
    // 句界：段尾，或后跟空白+大写（含数字）
    if (/^\s*$/.test(rest) || /^\s+[A-ZÀ-ÖØ-ÞA-Z0-9]/.test(rest)) {
      const sent = text.slice(last, end).trim();
      if (sent) out.push(sent);
      last = end;
    }
  }
  const tail = text.slice(last).trim();
  if (tail) out.push(tail);
  return out;
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

  // 重叠合并 + 语义重切：仅字幕来源为外部文件（subtitleSource === "file"）时默认启用。
  // YouTube 自动字幕是渐进式滑动窗口输出（每条 cue 重复上条尾部 + 新增内容，常带 10ms 回声段），
  // 不清理会导致 TTS 反复朗读同一句。无重叠的干净字幕完全幂等。
  const mergeOverlap = importCfg?.mergeOverlap ?? ctx.input?.task?.subtitleSource === "file";
  if (mergeOverlap) {
    const before = segments.length;
    const merged = mergeOverlapSegments(segments);
    if (merged.mergedCount > 0) {
      emitLog(
        taskDir,
        `[Import Subtitle] Overlap merge: ${before} → ${merged.segments.length} segs (merged ${merged.mergedCount})`,
      );
      segments = splitBySentences(merged.segments);
      emitLog(
        taskDir,
        `[Import Subtitle] Sentence split: ${merged.segments.length} → ${segments.length} segs`,
      );
      // TTS 细分：长句（> maxSegmentWords 词）先按逗号切，无逗号则从后往前切
      const maxWords = importCfg?.maxSegmentWords ?? 10;
      const beforeLen = segments.length;
      segments = splitForTTS(segments, maxWords);
      emitLog(
        taskDir,
        `[Import Subtitle] TTS split: ${beforeLen} → ${segments.length} segs (max ${maxWords} words)`,
      );
    } else {
      segments = merged.segments;
      emitLog(taskDir, "[Import Subtitle] No overlapping cues detected, skip merge");
    }
  }

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
