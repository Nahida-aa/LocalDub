import { FrameResult, OcrBoxResult } from "@repo/subtitle-ocr/types";
import { OcrAfterAdjustArgs } from "@repo/core/input/types";
import { Segment, SegmentWithAdjusted } from "@repo/core/ml/subtitle_ocr/types";
import { BoxAdjustedArgs } from "@repo/core/ml/subtitle_ocr/input";

type PolygonMetrics = [[number, number], [number, number], [number, number]];

function polygon_metrics(box: number[][]): PolygonMetrics {
  if (!box || box.length < 2)
    return [
      [0, 0],
      [0, 0],
      [0, 0],
    ];
  const n = box.length;
  const [minXy, maxXy, sum] = box.reduce(
    ([min, max, s], [x, y]) => [
      [Math.min(min[0], x), Math.min(min[1], y)],
      [Math.max(max[0], x), Math.max(max[1], y)],
      [s[0] + x, s[1] + y],
    ],
    [
      [Infinity, Infinity],
      [-Infinity, -Infinity],
      [0, 0],
    ] as [number[], number[], number[]],
  );
  return [
    [minXy[0], maxXy[0]],
    [minXy[1], maxXy[1]],
    [sum[0] / n, sum[1] / n],
  ];
}

// 一次 fold 算出所有点的 x/y 最小最大（对应 Rust 的 points_range）
function points_range(pts: number[][]): [[number, number], [number, number]] {
  if (!pts || pts.length === 0)
    return [
      [0, 0],
      [0, 0],
    ];
  const [minXy, maxXy] = pts.reduce(
    ([min, max], [x, y]) => [
      [Math.min(min[0], x), Math.min(min[1], y)],
      [Math.max(max[0], x), Math.max(max[1], y)],
    ],
    [
      [Infinity, Infinity],
      [-Infinity, -Infinity],
    ] as [number[], number[]],
  );
  return [
    [minXy[0], maxXy[0]],
    [minXy[1], maxXy[1]],
  ];
}

/*
 * 先给时间置0
 */
export function aggregate_boxes(boxes: OcrBoxResult[]): FrameResult {
  if (boxes.length === 0) {
    return {
      text: "",
      confidence: 0,
      x_range: [0, 0],
      y_range: [0, 0],
      boxes: [],
      timestamp: 0,
    };
  }
  if (boxes.length === 1) {
    const [xRange, yRange, center] = polygon_metrics(boxes[0].box);
    return {
      text: boxes[0].text,
      confidence: boxes[0].text_confidence,
      x_range: xRange,
      y_range: yRange,
      boxes: [
        {
          text: boxes[0].text,
          text_confidence: boxes[0].text_confidence,
          box: boxes[0].box,
          x_range: xRange,
          y_range: yRange,
          center,
        },
      ],
      timestamp: 0,
    };
  }
  const yRanges = boxes.map((l) => {
    const ys = l.box.map((p) => p[1]);
    return { min: Math.min(...ys), max: Math.max(...ys) };
  });
  let sameLine = false;
  for (let a = 0; a < yRanges.length - 1 && !sameLine; a++) {
    for (let b = a + 1; b < yRanges.length && !sameLine; b++) {
      if (yRanges[a].max >= yRanges[b].min && yRanges[b].max >= yRanges[a].min) sameLine = true;
    }
  }
  const avgConf = boxes.reduce((s, l) => s + l.text_confidence, 0) / boxes.length;
  const lineMetrics = boxes.map((l) => polygon_metrics(l.box));
  const [combinedXRange, combinedYRange] = points_range(boxes.flatMap((l) => l.box));
  return {
    text: boxes.map((l) => l.text).join(sameLine ? " " : "\n"),
    confidence: avgConf,
    x_range: combinedXRange,
    y_range: combinedYRange,
    boxes: boxes.map((l, i) => ({
      text: l.text,
      text_confidence: l.text_confidence,
      box: l.box,
      x_range: lineMetrics[i][0],
      y_range: lineMetrics[i][1],
      center: lineMetrics[i][2],
    })),
    timestamp: 0,
  };
}

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

export const build_ocr_frames_box_adjust = (
  ocrFrames: FrameResult[],
  yStats: YStats,
  { boxAdjustedThreshold = 0.5 }: BoxAdjustedArgs,
) =>
  ocrFrames.map((f) => ({
    ...f,
    boxes: f.boxes.map((l) => {
      if (!l.text.trim())
        return {
          ...l,
          top: 0,
          bottom: 0,
          top_offset_ratio: 0,
          bot_offset_ratio: 0,
          height: 0,
          height_ratio: 0,
          is_outlier: false,
          adjustedConfidence: l.text_confidence,
        };
      const top = l.y_range[0];
      const bottom = l.y_range[1];
      const height = bottom - top;
      // 这一行上边界，相对典型上边界，偏离了多少个行高
      const topOR =
        yStats.medianHeight > 0 ? Math.abs(top - yStats.mode[0]) / yStats.medianHeight : 0;
      const botOR =
        yStats.medianHeight > 0 ? Math.abs(bottom - yStats.mode[1]) / yStats.medianHeight : 0;
      const heightRatio =
        yStats.medianHeight > 0 ? Math.round((height / yStats.medianHeight) * 100) / 100 : 0;
      const bandDrift = Math.max(topOR, botOR); // 上下边界偏离取大的
      const noisePenalty = Math.min(
        1,
        Math.max(0, (bandDrift - 1.0) * 0.5) + // band 偏离 >1 行高才罚
          Math.abs(1 - heightRatio) * 0.3,
      );
      const adjustedConfidence = Math.round(l.text_confidence * (1 - noisePenalty) * 100) / 100;
      const isOutlier = adjustedConfidence < boxAdjustedThreshold;
      return {
        ...l,
        top,
        bottom,
        top_offset_ratio: Math.round(topOR * 100) / 100,
        bot_offset_ratio: Math.round(botOR * 100) / 100,
        height: Math.round(height * 10) / 10,
        height_ratio: heightRatio,
        is_outlier: isOutlier,
        adjustedConfidence,
      };
    }),
  }));
type OcrFramesLineAdjustFrame = ReturnType<typeof build_ocr_frames_box_adjust>[number];

export const get_ocr_frames_line_filtered = (
  ocrFramesLineAdjustFrames: OcrFramesLineAdjustFrame[],
) =>
  ocrFramesLineAdjustFrames.flatMap((f) => {
    const cleanLines = f.boxes.filter((l) => !l.is_outlier);
    if (cleanLines.length === 0) return [];
    if (cleanLines.length === f.boxes.length) return [f as FrameResult];
    const rebuilt = aggregate_boxes(
      cleanLines.map((l) => ({
        text: l.text,
        text_confidence: l.text_confidence,
        box: l.box,
        x_range: l.x_range,
        y_range: l.y_range,
        center: l.center,
      })),
    );
    return [
      {
        ...f,
        text: rebuilt.text,
        confidence: rebuilt.confidence,
        x_range: rebuilt.x_range,
        y_range: rebuilt.y_range,
        boxes: rebuilt.boxes,
      } as FrameResult,
    ];
  });

export function computeSegmentAdjustments(
  segments: Segment[],
  frameResults: FrameResult[],
  yStats: { avg: [number, number]; mode: [number, number] },
  videoHeight: number,
  {
    isoThresholdMs = 1500,
    adjustYWeight = 0.8,
    adjustIsoWeight = 0.2,
    adjustYFactor = 0.08,
  }: OcrAfterAdjustArgs,
): SegmentWithAdjusted[] {
  if (segments.length === 0 || (!yStats.avg[0] && yStats.avg[1] === 0)) return segments;

  const avgCentroid = (yStats.avg[0] + yStats.avg[1]) / 2;

  // build sorted non-empty frame timestamps for isolation search
  const nonEmptyTs = frameResults
    .filter((f) => f.text && f.x_range && f.y_range)
    .map((f) => f.timestamp)
    .sort((a, b) => a - b);

  return segments.map((seg) => {
    if (seg.frameCount === undefined || seg.confidence === undefined) return seg;

    // Y penalty: centroid offset relative to video height
    let yPenalty = 0;
    if (seg.box_y) {
      const centroid = (seg.box_y[0] + seg.box_y[1]) / 2;
      const offset = Math.abs(centroid - avgCentroid);
      yPenalty = Math.min(1, offset / (videoHeight * adjustYFactor));
    }

    // Isolation penalty: only for single-frame segments
    let isoPenalty = 0;
    if (seg.frameCount === 1) {
      const mid = (seg.start + seg.end) / 2;
      const nonEmptyBefore = [...nonEmptyTs].reverse().find((t) => t < mid);
      const nonEmptyAfter = nonEmptyTs.find((t) => t > mid);
      const gapBefore = nonEmptyBefore !== undefined ? mid - nonEmptyBefore : Infinity;
      const gapAfter = nonEmptyAfter !== undefined ? nonEmptyAfter - mid : Infinity;
      const nearestGap = Math.min(gapBefore, gapAfter);
      isoPenalty = Math.min(1, nearestGap / isoThresholdMs);
    }

    const totalPenalty = adjustYWeight * yPenalty + adjustIsoWeight * isoPenalty;
    const adjustedConfidence = seg.confidence * Math.max(0, 1 - totalPenalty);

    return {
      ...seg,
      adjustedConfidence: Math.round(adjustedConfidence * 100) / 100,
      yPenalty: Math.round(yPenalty * 100) / 100,
      isoPenalty: Math.round(isoPenalty * 100) / 100,
    };
  });
}
