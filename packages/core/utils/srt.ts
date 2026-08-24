import { TaskCtx } from "../context/context";
import { srtTime } from "./utils";
import { writeFile } from "./fileOps";
import { Timing } from "../stages/mix_audio/types";
import { TranslateSegment } from "../stages/05_translate/out";

/**
 *  按标点切分长句（,, ,, 。, ? 等），但保护配对符号（《》、「」 里的内容不被切开）
 */
function splitProtected(text: string): string[] {
  const PUNCTUATION = new Set(["，", ",", "；", ";", "：", ":", "。", "?", "？", "!", "！", "、"]);
  const PROTECTED_PAIRS: Record<string, string> = {
    "《": "》",
    "（": "）",
    "【": "】",
    "「": "」",
    "『": "』",
  };
  const segs: string[] = [];
  let buf: string[] = [],
    inside: string | null = null;
  for (const ch of text) {
    if (!inside && ch in PROTECTED_PAIRS) {
      inside = PROTECTED_PAIRS[ch];
      buf.push(ch);
      continue;
    }
    if (inside && ch === inside) {
      inside = null;
      buf.push(ch);
      continue;
    }
    if (!inside && PUNCTUATION.has(ch)) {
      const s = buf.join("").trim();
      if (s) segs.push(s);
      buf = [];
      continue;
    }
    buf.push(ch);
  }
  const tail = buf.join("").trim();
  if (tail) segs.push(tail);
  return segs;
}
/**
 * 修复引号被切到下一段的问题：如果一段以右引号开头，就合并到上一段末尾
 */
function attachClosingQuotes(segs: string[]): string[] {
  const fixed: string[] = [];
  const CLOSING_QUOTES = new Set(['"', "'", "」", "』", "》", "）", "】", "\u201d", "\u2019", "]"]);
  for (const s of segs) {
    if (s && CLOSING_QUOTES.has(s[0]) && fixed.length) {
      fixed[fixed.length - 1] = `${fixed[fixed.length - 1]}${s}`.trim();
    } else {
      fixed.push(s.trim());
    }
  }
  return fixed;
}
/**
 * 合并 <5 字符的超短段到下一段（避免字幕一闪而过）
 */
function mergeShort(segs: string[]): string[] {
  const merged: string[] = [];
  let i = 0;
  while (i < segs.length) {
    const cur = segs[i];
    if (cur.trim().length < 5 && i + 1 < segs.length) {
      segs[i + 1] = `${cur}${segs[i + 1]}`.trim();
      i++;
      continue;
    }
    merged.push(cur);
    i++;
  }
  return merged;
}
function stripTrailingPunct(segs: string[]): string[] {
  return segs
    .map((s) => {
      const t = s.trim();
      if (!t) return "";
      if (t.endsWith("，") || t.endsWith(",") || t.endsWith("。")) return t.slice(0, -1);
      return t.replace(/\s+/g, " ").trim();
    })
    .filter(Boolean);
}

/** 先保留 */
function splitSubtitle(text: string): string[] {
  if (!text.trim()) return [];
  const segs = stripTrailingPunct(mergeShort(attachClosingQuotes(splitProtected(text))));
  return segs.length ? segs : [text.trim()];
}

/**
 * 按 fragment 分配时间的写入逻辑（就是从原 writeSrt 抽出来的）
 * 先保留
 */
function writeSrtFragments(
  lines: string[],
  idx: { value: number },
  start: number,
  end: number,
  fragments: string[],
) {
  const totalDuration = end - start;
  const weights = fragments.map((f) => Math.max(1, f.replace(/\s/g, "").length));
  const totalWeight = weights.reduce((a, b) => a + b, 0);
  let cursor = start,
    allocated = 0;
  for (let f = 0; f < fragments.length; f++) {
    const share =
      f < fragments.length - 1
        ? Math.max(
            200,
            Math.min(
              Math.round((totalDuration * weights[f]) / totalWeight),
              totalDuration - allocated - 100,
            ),
          )
        : Math.max(100, totalDuration - allocated);
    lines.push(
      String(idx.value),
      `${srtTime(cursor)} --> ${srtTime(cursor + share)}`,
      fragments[f],
      "",
    );
    cursor += share;
    allocated += share;
    idx.value++;
  }
}

export function writeSrt(
  segments: (Timing | TranslateSegment)[],
  ctx: TaskCtx,
  outputPath: string,
  useSource?: boolean,
) {
  console.log(`Writing SRT length: ${segments.length}...`);
  const lines: string[] = [];
  let idx = 1;
  for (const item of segments) {
    const start = Math.floor("actual_start" in item ? item?.actual_start : item.start_ms);
    const end = Math.floor("actual_end" in item ? item?.actual_end : item.end_ms);
    // if (end <= start) continue;

    const text = useSource ? (item.text || "").trim() : (item.dst || item.text || "").trim();
    if (!text) continue;
    // 默认一条 SRT（之前由 splitSubtitle 切分的逻辑已抽成 writeSrtFragments）
    lines.push(String(idx), `${srtTime(start)} --> ${srtTime(end)}`, text, "");
    idx++;
  }

  const content = lines.join("\n");
  // 预检：在交给 ffmpeg 前拦截内容问题，避免其报出含糊的 "Unable to open"
  validateSrtContent(content, outputPath);

  writeFile(outputPath, content, ctx);
}

const SRT_TIME_RE = /^\d{2}:\d{2}:\d{2},\d{3} --> \d{2}:\d{2}:\d{2},\d{3}$/;

/**
 * 校验 SRT 文本内容是否可被 ffmpeg/libass 的 subtitles 滤镜正常读取。
 * ffmpeg 在字幕文件内容有问题时往往只报模糊的 "Unable to open"，
 * 这里在写盘前给出明确的错误信息（文件名 + 块号 + 原因）。
 */
export function validateSrtContent(content: string, filePath: string): void {
  const rawLines = content.split("\n");
  const blocks: string[][] = [];
  let current: string[] = [];
  for (const line of rawLines) {
    if (line.trim() === "") {
      if (current.length) blocks.push(current);
      current = [];
    } else {
      current.push(line);
    }
  }
  if (current.length) blocks.push(current);

  if (blocks.length === 0) {
    throw new Error(`SRT 预检失败 (${filePath}): 文件为空，没有任何字幕块`);
  }

  blocks.forEach((block, i) => {
    const blockNo = i + 1;
    if (block.length < 2) {
      throw new Error(
        `SRT 预检失败 (${filePath}): 第 ${blockNo} 块结构非法，期望 "序号 / 时间轴 / 文本"，实际 ${block.length} 行`,
      );
    }
    if (!SRT_TIME_RE.test(block[1])) {
      throw new Error(
        `SRT 预检失败 (${filePath}): 第 ${blockNo} 块时间轴格式非法: "${block[1]}" (应为 HH:MM:SS,mmm --> HH:MM:SS,mmm)`,
      );
    }
    const [s, e] = block[1].split(" --> ").map((t) => {
      const [h, m, rest] = t.split(":");
      const [s2, ms] = rest.split(",");
      return (+h * 3600 + +m * 60 + +s2) * 1000 + +ms;
    });
    if (!(e > s)) {
      throw new Error(
        `SRT 预检失败 (${filePath}): 第 ${blockNo} 块时间轴非法，结束时间必须晚于开始时间 (${block[1]})`,
      );
    }
    // 文本行不得包含 ffmpeg subtitles 滤镜无法处理的 NUL 字符
    for (let li = 2; li < block.length; li++) {
      if (block[li].includes("\u0000")) {
        throw new Error(`SRT 预检失败 (${filePath}): 第 ${blockNo} 块文本含非法 NUL 字符`);
      }
    }
  });
}
