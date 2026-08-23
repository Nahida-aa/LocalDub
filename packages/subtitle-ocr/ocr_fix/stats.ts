import { FrameResult } from "../types";

export type YStats = {
  avg: [number, number];
  mode: [number, number];
  median: [number, number];
  avgHeight: number;
  medianHeight: number;
  modeHeight: number;
};

export function computeBoxYStats(frames: FrameResult[]): YStats {
  const boxes = frames.flatMap((f) => f.boxes ?? []).filter((l) => l.text.trim());
  if (boxes.length === 0)
    return {
      avg: [0, 0],
      mode: [0, 0],
      median: [0, 0],
      avgHeight: 0,
      medianHeight: 0,
      modeHeight: 0,
    };

  const boxYs = boxes.map((l) => l.y_range as [number, number]);

  const avgTop = Math.round(boxYs.reduce((s, [t]) => s + t, 0) / boxYs.length);
  const avgBtm = Math.round(boxYs.reduce((s, [, b]) => s + b, 0) / boxYs.length);
  const avgHeight = Math.round(
    boxes.reduce((s, l) => s + (l.y_range[1] - l.y_range[0]), 0) / boxes.length,
  );

  const heights = boxes.map((l) => l.y_range[1] - l.y_range[0]).sort((a, b) => a - b);
  const mid = Math.floor(heights.length / 2);
  const medianHeight =
    heights.length % 2 === 0 ? Math.round((heights[mid - 1] + heights[mid]) / 2) : heights[mid];

  // 位置中位数: 所有 top 排序取中位数、所有 bottom 排序取中位数
  const tops = boxYs.map(([t]) => t).sort((a, b) => a - b);
  const btms = boxYs.map(([, b]) => b).sort((a, b) => a - b);
  const medianOf = (arr: number[]) => {
    const m = Math.floor(arr.length / 2);
    return arr.length % 2 === 0 ? Math.round((arr[m - 1] + arr[m]) / 2) : arr[m];
  };
  const median: [number, number] = [medianOf(tops), medianOf(btms)];

  // 高度众数: 出现最频繁的行高
  const heightCounts = new Map<number, number>();
  let modeHeightCount = 0;
  let modeHeight = heights[0];
  for (const h of heights) {
    const c = (heightCounts.get(h) ?? 0) + 1;
    heightCounts.set(h, c);
    if (c > modeHeightCount) {
      modeHeightCount = c;
      modeHeight = h;
    }
  }

  const counts = new Map<string, { count: number; pair: [number, number] }>();
  let maxCount = 0;
  let mode: [number, number] = boxYs[0];
  for (const pair of boxYs) {
    const key = `${pair[0]},${pair[1]}`;
    const entry = counts.get(key) ?? { count: 0, pair };
    entry.count++;
    counts.set(key, entry);
    if (entry.count > maxCount) {
      maxCount = entry.count;
      mode = pair;
    }
  }

  return { avg: [avgTop, avgBtm], mode, median, avgHeight, medianHeight, modeHeight };
}
