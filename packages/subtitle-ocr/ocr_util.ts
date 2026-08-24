import { OcrBoxResult, FrameResult } from "./types";

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
 *
 * 泛型版：输入框若带有 OcrBoxResult 之外的额外字段（如 box-adjust 阶段标注的
 * top_offset_ratio / is_outlier 等），聚合后这些字段会按各子框原样透传到对应输出框，
 * 使聚合结果也能保留诊断信息。仅传标准 OcrBoxResult 时退化为普通 FrameResult。
 */
type BoxExtra<T> = Omit<T, keyof OcrBoxResult>;

function mapBoxToAgg<T extends OcrBoxResult>(
  l: T,
  lineMetrics: PolygonMetrics,
): OcrBoxResult & BoxExtra<T> {
  // 解构出会被重算的字段（x_range/y_range/center）以及基础字段，
  // rest 仅剩输入框的额外字段，透传下去。
  const { text, text_confidence, box, x_range, y_range, center, ...rest } = l;
  return {
    text,
    text_confidence,
    box,
    x_range: lineMetrics[0],
    y_range: lineMetrics[1],
    center: lineMetrics[2],
    ...rest,
  } as OcrBoxResult & BoxExtra<T>;
}

export function aggregate_boxes<T extends OcrBoxResult>(
  boxes: T[],
): FrameResult & { boxes: Array<OcrBoxResult & BoxExtra<T>> } {
  if (boxes.length === 0) {
    return {
      text: "",
      text_confidence: 0,
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
      text_confidence: boxes[0].text_confidence,
      x_range: xRange,
      y_range: yRange,
      boxes: [mapBoxToAgg(boxes[0], [xRange, yRange, center])],
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
    text_confidence: avgConf,
    x_range: combinedXRange,
    y_range: combinedYRange,
    boxes: boxes.map((l, i) => mapBoxToAgg(l, lineMetrics[i])),
    timestamp: 0,
  };
}
