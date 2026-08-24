import { spawnSync } from "node:child_process";

/**
 * 探测媒体 (音频/视频) 时长, 返回毫秒。
 *
 * 注意: ffprobe `format=duration` 单位固定为秒 (浮点, 精度到微秒),
 * 无法直接输出毫秒, 这里统一 round(秒 × 1000) 换算。
 */
export function probeDurationMs(mediaPath: string): number {
  const r = spawnSync(
    "ffprobe",
    ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", mediaPath],
    { timeout: 15_000, encoding: "utf-8" },
  );
  return Math.round(parseFloat(r.stdout?.trim() || "0") * 1000);
}

export interface FrameRate {
  numerator: number;
  denominator: number;
}

/** 用 ffprobe 读取视频帧率，失败时返回默认值 { numerator: 30, denominator: 1 } */
export function probeFrameRate(
  videoPath: string,
  defaultValue: FrameRate = { numerator: 30, denominator: 1 },
): FrameRate {
  try {
    const r = spawnSync(
      "ffprobe",
      [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=r_frame_rate",
        "-of",
        "csv=p=0",
        videoPath,
      ],
      { timeout: 10_000 },
    );
    if (r.status !== 0) return defaultValue;
    const output = r.stdout.toString().trim();
    if (!output || !output.includes("/")) return defaultValue;
    // e.g. "30000/1001" → { numerator: 30000, denominator: 1001 }
    const [numStr, denStr] = output.split("/");
    const num = parseInt(numStr, 10);
    const den = parseInt(denStr, 10);
    if (isNaN(num) || isNaN(den) || den === 0) return defaultValue;
    return { numerator: num, denominator: den };
  } catch {
    console.warn(`probeFrameRate ${videoPath} hit defaultValue:`, defaultValue);
    return defaultValue;
  }
}
