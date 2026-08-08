import { FrameResult } from "../types";

export const hasNearbySameText = (
  rawFrames: FrameResult[],
  i: number,
  f: FrameResult,
  resampleRangeMs = 500,
) => {
  return rawFrames.some(
    (other, j) =>
      j !== i &&
      other.text === f.text &&
      Math.abs(other.timestamp - f.timestamp) <= resampleRangeMs,
  );
};

// 问题: end2fps 抽帧在某些高置信度字幕附近可能留大空隙（相邻同文本帧间距 > RESAMPLE_RANGE_MS），
//       导致后续字幕合并时间边界不准。这里在空隙区间按 RESAMPLE_STEP_MS 步长补抽帧并 OCR，
//       把更多帧并入 rawFrames，提升时间覆盖密度。
// 注意: 仅当某帧"高置信度 + 附近无相同文本帧"才视为孤立点，才触发补抽。
export const collect_resample_candidate_ms = (frames: FrameResult[]) => {
  const RESAMPLE_CONF_THRESH = 0.6; // 仅对高置信度帧补抽，低置信噪声帧不补
  const RESAMPLE_STEP_MS = 100; // 补抽步长
  const RESAMPLE_RANGE_MS = 500; // 在孤立帧 ±500ms 内补抽

  const isolatedInfos: string[] = [];
  const candidateTs = new Set<number>();
  for (const [i, f] of frames.entries()) {
    if (!f.text || f.confidence < RESAMPLE_CONF_THRESH) continue;
    // 若附近已有相同文本的帧，说明不是孤立点，无需补抽
    const is_hasNearbySameText = hasNearbySameText(frames, i, f, RESAMPLE_RANGE_MS);
    if (is_hasNearbySameText) continue;
    const prevTs = i > 0 ? frames[i - 1].timestamp : -Infinity;
    const nextTs = i < frames.length - 1 ? frames[i + 1].timestamp : Infinity;
    const gapBefore = f.timestamp - prevTs;
    const gapAfter = nextTs - f.timestamp;
    // 记录孤立点信息用于日志（gapBefore/gapAfter 是该帧与 rawFrames 中相邻帧的时间空隙，仅展示用，不参与孤立判定）
    isolatedInfos.push(
      `  tms=${f.timestamp}ms  text="${f.text.slice(0, 30)}"  conf=${f.confidence}  gapBefore=${gapBefore}ms  gapAfter=${gapAfter}ms`,
    );
    // 在孤立帧两侧 ±RESAMPLE_RANGE_MS 内，按步长收集候选时间戳
    for (
      let t = f.timestamp - RESAMPLE_RANGE_MS;
      t <= f.timestamp + RESAMPLE_RANGE_MS;
      t += RESAMPLE_STEP_MS
    ) {
      if (t >= 0) candidateTs.add(t);
    }
  }
  return [candidateTs, isolatedInfos] as const;
};
