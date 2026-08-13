import { SplitAudioSegment, SplitAudioTiming } from "./out";

/**
 * 给各段时间轴加前后 padding (默认前 100ms / 后 300ms), 避免切块时把语音截断。
 *
 * 规则:
 * - 每段独立计算 start/end 的 padding 量, 相邻段之间的空白 (gap) 越充足则越接满默认值;
 * - gap 不足时按比例分摊 start/end, 保证不越过相邻段;
 * - minGap=50ms 之下的缝直接取中点, 避免与相邻段重叠。
 * 返回新数组, 不修改原数组。
 */
export function padSegments(
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
