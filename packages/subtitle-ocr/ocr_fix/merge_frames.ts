import { FrameResult, OcrSegment, SegmentFrame } from "@repo/subtitle-ocr/types";
import { MergeFramesArgs } from "@repo/subtitle-ocr/args";
import { srtTime } from "@repo/util/time";

/**
 * 编辑距离算法: levenshtein，计算两个字符串之间需要多少次插入/删除/替换才能变成对方。
 * ```ts
 * edit_distance("陆", "陆执巡") = 2
 * ```
 * 陆 → 陆执巡: 插入 "执" + 插入 "巡" = 2 次操作
 * ```ts
 * edit_distance("陆", "这其中是不是有什么误会") = 9
 * ```
 * 每个字都要替换 = 9 次操作
 */
export function edit_distance(a: string, b: string): number {
  const m = a.length,
    n = b.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
  for (let i = 0; i <= m; i++) dp[i][0] = i;
  for (let j = 0; j <= n; j++) dp[0][j] = j;
  for (let i = 1; i <= m; i++)
    for (let j = 1; j <= n; j++)
      dp[i][j] =
        a[i - 1] === b[j - 1]
          ? dp[i - 1][j - 1]
          : Math.min(dp[i - 1][j], dp[i][j - 1], dp[i - 1][j - 1]) + 1;
  return dp[m][n];
}

function overlap(a?: [number, number], b?: [number, number]): boolean {
  if (!a || !b) return false;
  return a[0] < b[1] && b[0] < a[1];
}

function isSubstringOf(a: string, b: string): boolean {
  if (!a || !b || a.length === b.length) return false;
  return a.length < b.length ? b.includes(a) : a.includes(b);
}

function avgConfidence(confidences: number[]): number | undefined {
  return confidences.length > 0
    ? confidences.reduce((a, b) => a + b, 0) / confidences.length
    : undefined;
}

function mergeConfidence(a?: number, b?: number): number | undefined {
  if (a === undefined) return b;
  if (b === undefined) return a;
  return (a + b) / 2;
}

const normalize = (s: string) => s.replace(/\s+/g, "");

/**
 * second pass: merge adjacent segments where text is a substring of the other
 * and Y positions overlap (handles OCR single-character hallucination like 身→绝不起身)
 */
function mergeSubstringSegments(segments: OcrSegment[]): OcrSegment[] {
  for (let i = segments.length - 1; i > 0; i--) {
    const prev = segments[i - 1];
    const cur = segments[i];
    if (!overlap(prev.y_range, cur.y_range)) continue;
    if (isSubstringOf(prev.text, cur.text)) {
      segments[i - 1] = {
        text: cur.text,
        start_ms: prev.start_ms,
        end_ms: cur.end_ms,
        y_range: cur.y_range,
        text_confidence: mergeConfidence(prev.text_confidence, cur.text_confidence),
        frameCount: (prev.frameCount ?? 1) + (cur.frameCount ?? 1),
      };
      segments.splice(i, 1);
    } else if (isSubstringOf(cur.text, prev.text)) {
      segments[i - 1] = {
        text: prev.text,
        start_ms: prev.start_ms,
        end_ms: cur.end_ms,
        y_range: prev.y_range,
        text_confidence: mergeConfidence(prev.text_confidence, cur.text_confidence),
        frameCount: (prev.frameCount ?? 1) + (cur.frameCount ?? 1),
      };
      segments.splice(i, 1);
    }
  }
  return segments;
}

export function mergeFrames(
  frames: FrameResult[],
  args: MergeFramesArgs,
): {
  text: string;
  segments: OcrSegment[];
} {
  const segments: OcrSegment[] = [];
  let currentText = "";
  let currentStart = 0;
  let currentBoxY: [number, number] | undefined;
  let gapStart = 0;
  let currentConfidences: number[] = [];
  let currentFrames: SegmentFrame[] = [];
  let currentEnd = 0;

  for (const f of frames) {
    // ─── A: 空帧 → 标记 gap ───
    if (!f.text) {
      if (currentText && !gapStart) gapStart = f.timestamp;
      continue;
    }
    // ─── B: gap 恢复检查（空帧后同 text 恢复）───
    if (gapStart > 0) {
      const gapMs = f.timestamp - gapStart;
      if (
        gapMs <= 1500 &&
        (normalize(f.text) === normalize(currentText) ||
          isSubstringOf(f.text, currentText) ||
          isSubstringOf(currentText, f.text))
      ) {
        // B1: gap 恢复成功 → 合并回当前段
        currentConfidences.push(f.text_confidence);
        currentEnd = f.timestamp;
        gapStart = 0;
        continue;
      }
      // B2: gap 恢复失败 → flush 当前段，重置
      segments.push({
        text: currentText,
        start_ms: currentStart,
        end_ms: gapStart,
        y_range: currentBoxY,
        text_confidence: avgConfidence(currentConfidences),
        frameCount: currentConfidences.length,
        frames: currentFrames,
      });
      currentText = "";
      currentStart = 0;
      currentBoxY = undefined;
      gapStart = 0;
      currentConfidences = [];
      currentFrames = [];
    }
    // ─── C: text 比较 ───
    if (!currentText || normalize(f.text) !== normalize(currentText)) {
      // C1: 不同 text → flush 旧段，开始新段
      if (currentText) {
        segments.push({
          text: currentText,
          start_ms: currentStart,
          end_ms: currentEnd,
          y_range: currentBoxY,
          text_confidence: avgConfidence(currentConfidences),
          frameCount: currentConfidences.length,
          frames: currentFrames,
        });
      }
      currentText = f.text;
      currentStart = f.timestamp;
      currentEnd = f.timestamp;
      currentBoxY = f.y_range;
      currentConfidences = [f.text_confidence];
      currentFrames = [
        { timestamp: f.timestamp, text: f.text, text_confidence: f.text_confidence },
      ];
    } else {
      // C2: 同 text → 延续当前段
      currentConfidences.push(f.text_confidence);
      currentEnd = f.timestamp;
      currentFrames.push({
        timestamp: f.timestamp,
        text: f.text,
        text_confidence: f.text_confidence,
      });
    }
  }
  // ─── D: 循环结束 flush 最后一段 ───
  if (currentText) {
    const lastTs = gapStart > 0 ? gapStart : currentEnd;
    segments.push({
      text: currentText,
      start_ms: currentStart,
      end_ms: lastTs,
      y_range: currentBoxY,
      text_confidence: avgConfidence(currentConfidences),
      frameCount: currentConfidences.length,
      frames: currentFrames,
    });
  }

  // ─── Pass 1: substring merge ───
  if (args.mergeSubstring) {
    mergeSubstringSegments(segments);
  }

  // ─── Pass 2: A-B-C triplet 噪声消除 ───
  // third pass: A-B-C triplet where A.text == C.text and B is a short hallucination
  // (handles patterns like "嗯发财了" → "菌" → "嗯发财了", or same-text segments
  // split by a one-word noise like "娘带着我们门爬了七座山才到")
  for (let i = 0; i < segments.length - 2; i++) {
    const a = segments[i];
    const b = segments[i + 1];
    const c = segments[i + 2];
    if (
      edit_distance(a.text, c.text) <= 2 &&
      overlap(a.y_range, b.y_range) &&
      overlap(b.y_range, c.y_range)
    ) {
      const durB = b.end_ms - b.start_ms;
      const isShort = durB <= 1000;
      // 中间段是一个 OCR 噪声：要么它本身就很短（<=1000ms），要么
      // 它和 a/c 的文本差异很小且长度差不多（中间插入一个字的噪声）
      const bNearA =
        edit_distance(b.text, a.text) <= 2 && Math.abs(b.text.length - a.text.length) <= 2;
      const bNearC =
        edit_distance(b.text, c.text) <= 2 && Math.abs(b.text.length - c.text.length) <= 2;
      const isNoise = isShort || bNearA || bNearC;
      if (isNoise) {
        const mergedConf = [a.text_confidence, b.text_confidence, c.text_confidence].filter(
          (v): v is number => v !== undefined,
        );
        segments[i] = {
          text: a.text,
          start_ms: a.start_ms,
          end_ms: c.end_ms,
          y_range: a.y_range,
          text_confidence: avgConfidence(mergedConf),
          frameCount: (a.frameCount ?? 1) + (b.frameCount ?? 1) + (c.frameCount ?? 1),
          frames: [...(a.frames ?? []), ...(b.frames ?? []), ...(c.frames ?? [])],
        };
        segments.splice(i + 1, 2);
        i--; // re-check from this position
      }
    }
  }

  // ─── Pass 3: overlapping dedup ───
  // fourth pass: remove overlapping segments with similar text
  // (handles ASR segment overlap where end2fps scans produce duplicate
  // segments with lev-distant text like 干嘛/于嘛 in the same time window)
  dedupOverlap(segments, args.dedup_edit_distance);

  // ─── Pass 4: 同 text 相邻合并 ───
  // fifth pass: merge adjacent segments with the same normalized text
  // (handles A → noise → A cut by a frame gap where the triplet didn't fire
  // because noise was a single long segment, e.g. same subtitle resumed after
  // a punctuation/breath pause split the ASR slice)
  for (let i = segments.length - 1; i > 0; i--) {
    const prev = segments[i - 1];
    const cur = segments[i];
    if (normalize(prev.text) !== normalize(cur.text)) continue;
    const gap = cur.start_ms - prev.end_ms;
    if (gap < 0 || gap > 2000) continue; // 不重叠 + 间隔不超过 2s 才合并
    prev.end_ms = cur.end_ms;
    const mergedConf = [prev.text_confidence, cur.text_confidence].filter(
      (v): v is number => v !== undefined,
    );
    prev.text_confidence = avgConfidence(mergedConf);
    prev.frameCount = (prev.frameCount ?? 1) + (cur.frameCount ?? 1);
    segments.splice(i, 1);
    prev.frames = [...(prev.frames ?? []), ...(cur.frames ?? [])];
  }

  return {
    text: segments.map((s) => s.text).join(" "),
    segments: segments,
  };
}

export function dedupOverlap(segments: OcrSegment[], dedupLevenshtein = 1): OcrSegment[] {
  const TOUCH_GAP_MS = 500;
  for (let i = 0; i < segments.length; i++) {
    for (let j = i + 1; j < segments.length; j++) {
      const a = segments[i];
      const b = segments[j];
      if (!a || !b) continue;
      const gap = Math.max(a.start_ms, b.start_ms) - Math.min(a.end_ms, b.end_ms);
      const overlap = a.start_ms < b.end_ms && b.start_ms < a.end_ms;
      const touching = gap <= TOUCH_GAP_MS;
      if ((overlap || touching) && edit_distance(a.text, b.text) <= dedupLevenshtein) {
        segments[i] = {
          text: a.text.length >= b.text.length ? a.text : b.text,
          start_ms: Math.min(a.start_ms, b.start_ms),
          end_ms: Math.max(a.end_ms, b.end_ms),
          y_range: a.y_range,
          text_confidence: mergeConfidence(a.text_confidence, b.text_confidence),
          frameCount: (a.frameCount ?? 1) + (b.frameCount ?? 1),
          frames: [...(a.frames ?? []), ...(b.frames ?? [])],
        };
        segments.splice(j, 1);
        j--;
      }
    }
  }
  return segments;
}

export function fixOverlap(
  asrSegs: OcrSegment[],
  rawFrames: FrameResult[],
  ocrSegs: OcrSegment[],
  maxAdvanceMs = 500,
): OcrSegment[] {
  const fix = asrSegs.map((s) => ({ ...s }));
  const sorted = [...rawFrames].sort((a, b) => a.timestamp - b.timestamp);

  for (let i = 1; i < fix.length; i++) {
    const prev = fix[i - 1];
    const cur = fix[i];
    if (cur.start_ms >= prev.end_ms) continue;
    const overlapEnd = Math.min(prev.end_ms, cur.end_ms);
    for (const f of sorted) {
      if (f.timestamp < cur.start_ms) continue;
      if (f.timestamp > overlapEnd) break;
      const dCur = edit_distance(f.text, cur.text);
      const dPrev = edit_distance(f.text, prev.text);
      if (dCur <= 2 && dCur < dPrev) {
        prev.end_ms = f.timestamp;
        cur.start_ms = f.timestamp;
        break;
      }
    }
  }

  for (const seg of fix) {
    let bestOcr: OcrSegment | null = null;
    let bestOverlap = 0;
    for (const o of ocrSegs) {
      const overlap = Math.min(seg.end_ms, o.end_ms) - Math.max(seg.start_ms, o.start_ms);
      if (overlap > bestOverlap && edit_distance(seg.text, o.text) <= 2) {
        bestOverlap = overlap;
        bestOcr = o;
      }
    }
    if (bestOcr && seg.start_ms + maxAdvanceMs < bestOcr.start_ms) {
      seg.start_ms = bestOcr.start_ms;
    }
  }

  return fix;
}

/**
 * 从 ocr.json 的 segments 出发，按 segment confidence 过滤，生成 ocr_filtered.json 的结果。
 * 如果 segment 已带有 adjustedConfidence（Y 偏移 + 孤立惩罚后的置信度），则优先用它过滤。
 *
 * @param segments 来自 ocr.json.result.segments（mergeFrames 结果），或 computeSegmentAdjustments 的输出
 * @param textScore confidence 阈值，低于此值的 segment 会被丢弃。0 表示不过滤。
 * @returns 过滤后的 segments，以及被丢弃的数量
 */
export function toOcrFiltered(
  segments: OcrSegment[],
  textScore: number,
): { segments: OcrSegment[]; dropped: number } {
  if (!textScore || textScore <= 0) {
    return { segments: segments.map((s) => ({ ...s })), dropped: 0 };
  }
  const filtered = segments.filter((s) => {
    const score =
      (s as any).adjustedConfidence !== undefined
        ? (s as any).adjustedConfidence
        : s.text_confidence;
    return score === undefined || score >= textScore;
  });
  return {
    segments: filtered,
    dropped: segments.length - filtered.length,
  };
}
