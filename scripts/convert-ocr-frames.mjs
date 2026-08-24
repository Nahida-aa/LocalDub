#!/usr/bin/env node
// 将 ocr_frames.json（逐帧 OCR 历史格式）转换为当前 OcrFramesResult 结构:
//   { frames: FrameResult[], meta: { engine, device } }
//
// 帧部分是老格式的无损超集:
//   - lines[].box (4 点多边形) -> OcrBoxResult.box
//   - lines[].confidence       -> OcrBoxResult.text_confidence
//   - x_range / y_range / center 由 box 推导
//
// meta.engine / meta.device 优先从旧文件 (_engine/_device) 读取，缺失时回退默认值。
// 注意: 仅含 _ocr_segments (合并后字幕段) 的文件不含逐帧数据，无法转换为 OcrFramesResult。
//
// 用法:
//   node scripts/convert-ocr-frames.mjs <input> [output] [--dry-run]
//     <input>   必填，相对仓库根的路径
//     [output]  选填；不给则覆盖 <input>
//     --dry-run 纯打印转换结果到 stdout，不写任何文件

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const OCR_DEVICES = ["cpu", "cuda", "directml", "coreml", "rocm", "mps"];
const DEFAULT_ENGINE = "ort-cpp";
const DEFAULT_DEVICE = "cpu";

function polygonMetrics(box) {
  const xs = box.map((p) => p[0]);
  const ys = box.map((p) => p[1]);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  return {
    x_range: [minX, maxX],
    y_range: [minY, maxY],
    center: [
      xs.reduce((a, b) => a + b, 0) / xs.length,
      ys.reduce((a, b) => a + b, 0) / ys.length,
    ],
  };
}

// 聚合一组点的最小/最大 x/y 范围（对应 TS 的 points_range）
function pointsRange(points) {
  if (!points.length) return [[], []];
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  return [
    [Math.min(...xs), Math.max(...xs)],
    [Math.min(...ys), Math.max(...ys)],
  ];
}

function lineToBox(line) {
  const box = line.box;
  const { x_range, y_range, center } = polygonMetrics(box);
  return {
    text: line.text,
    text_confidence: line.confidence,
    box,
    x_range,
    y_range,
    center,
  };
}

function frameToResult(frame) {
  let boxes;
  if (Array.isArray(frame.lines) && frame.lines.length > 0) {
    // 老格式: lines[] 携带逐行 box
    boxes = frame.lines.map(lineToBox);
  } else if (Array.isArray(frame.box)) {
    // 老格式: 帧直接带单个 box 多边形（无 lines）
    const { x_range, y_range, center } = polygonMetrics(frame.box);
    boxes = [
      {
        text: frame.text,
        text_confidence: frame.confidence,
        box: frame.box,
        x_range,
        y_range,
        center,
      },
    ];
  }

  // 帧级 x_range / y_range: 优先用 bbox，否则从 boxes 聚合推导
  let x_range = frame.x_range;
  let y_range = frame.y_range;
  if (!x_range || !y_range) {
    if (frame.bbox && "left" in frame.bbox) {
      x_range = [frame.bbox.left, frame.bbox.right];
      y_range = [frame.bbox.top, frame.bbox.bottom];
    } else if (boxes && boxes.length > 0) {
      const [xr, yr] = pointsRange(boxes.flatMap((b) => b.box));
      x_range = xr;
      y_range = yr;
    }
  }

  return {
    text: frame.text,
    timestamp: frame.timestamp,
    confidence: frame.confidence,
    x_range,
    y_range,
    boxes: boxes ?? [],
  };
}

function parseArgs(argv) {
  const positional = [];
  let dryRun = false;
  for (const a of argv) {
    if (a === "--dry-run") dryRun = true;
    else if (a.startsWith("--")) throw new Error(`unknown option: ${a}`);
    else positional.push(a);
  }
  const [input, output] = positional;
  return { input, output, dryRun };
}

function main() {
  let input, output, dryRun;
  try {
    ({ input, output, dryRun } = parseArgs(process.argv.slice(2)));
  } catch (e) {
    console.error(e.message);
    process.exit(2);
  }
  if (!input) {
    console.error("usage: node scripts/convert-ocr-frames.mjs <input> [output] [--dry-run]");
    process.exit(2);
  }

  const inputPath = resolve(repoRoot, input);
  if (!existsSync(inputPath)) {
    console.error(`file not found: ${inputPath}`);
    process.exit(1);
  }

  const raw = JSON.parse(readFileSync(inputPath, "utf-8"));

  // 已经是新格式
  if (Array.isArray(raw.frames) && raw.meta) {
    const summary = `[ok] already OcrFramesResult (frames=${raw.frames.length}, engine=${raw.meta.engine}, device=${raw.meta.device})`;
    if (dryRun) {
      console.log(summary);
      console.log(JSON.stringify(raw, null, 2));
    } else {
      console.log(summary);
    }
    return;
  }

  // 老格式: 逐帧数据在 _frames_raw
  const oldFrames = raw._frames_raw;
  if (!Array.isArray(oldFrames)) {
    console.error(
      `unrecognized format: 仅支持含 "frames"+"meta" 或 "_frames_raw" 的逐帧文件。` +
        `该文件顶层 keys=${Object.keys(raw).join(", ")}` +
        (raw._ocr_segments ? "（含 _ocr_segments 的是合并后字幕段，非逐帧 OCR 数据，无法转换）" : ""),
    );
    process.exit(1);
  }

  const frames = oldFrames.map(frameToResult);
  const meta = {
    engine: raw._engine ?? raw.meta?.engine ?? DEFAULT_ENGINE,
    device: (raw._device ?? raw.meta?.device ?? DEFAULT_DEVICE),
  };
  if (!OCR_DEVICES.includes(meta.device)) meta.device = DEFAULT_DEVICE;
  const out = { frames, meta };

  const targetPath = output ? resolve(repoRoot, output) : inputPath;
  console.log(
    `[convert] ${oldFrames.length} frames -> OcrFramesResult (engine=${meta.engine}, device=${meta.device})`,
  );

  if (dryRun) {
    console.log(JSON.stringify(out, null, 2));
  } else {
    writeFileSync(targetPath, JSON.stringify(out, null, 2) + "\n", "utf-8");
    console.log(`[written] ${targetPath}`);
  }
}

main();
