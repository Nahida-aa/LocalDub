import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { to } from "@repo/shared/lib/utils/try";
import { write_log } from "@repo/util/log";

export const extract_frame = (ms: number, videoPath: string, framePath: string) => {
  const r = spawnSync(
    "ffmpeg",
    [
      "-y",
      "-ss",
      String(ms / 1000),
      "-i",
      videoPath,
      "-frames:v",
      "1",
      "-qscale:v",
      "2",
      framePath,
    ],
    { timeout: 15_000, encoding: "utf-8" },
  );
  if (r.error) {
    throw r.error;
  }
  if (r.status !== 0) {
    throw new Error(`ffmpeg exited with status ${r.status}`);
  }
  return r;
};

export const extract_frames = (
  sorted_ms: number[],
  videoPath: string,
  frameDir: string,
  taskDir: string,
) => {
  let extractCount = 0;
  for (const [i, ms] of sorted_ms.entries()) {
    const framePath = join(frameDir, `${ms.toString().padStart(7, "0")}.jpg`);
    const [r, err] = to(() => extract_frame(ms, videoPath, framePath));
    if (err) continue;
    extractCount++;

    if ((i + 1) % 50 === 0 || i === sorted_ms.length - 1) {
      write_log(taskDir, `Extracted ${i + 1}/${sorted_ms.length} frames`);
    }
  }
  write_log(taskDir, `Extracted ${extractCount}/${sorted_ms.length}  frames`);
  return extractCount;
};
