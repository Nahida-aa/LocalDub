#!/usr/bin/env bun
import { join, resolve } from "node:path";
import { existsSync, readdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const repoRoot = resolve(import.meta.dir, "..");
const WORKFOLDER = process.env["WORKFOLDER"]
  ? resolve(repoRoot, process.env["WORKFOLDER"])
  : join(repoRoot, "workfolder");

function ffprobeDuration(file: string): number | null {
  const res = spawnSync(
    "ffprobe",
    [
      "-v",
      "error",
      "-show_entries",
      "format=duration",
      "-of",
      "default=noprint_wrappers=1:nokey=1",
      file,
    ],
    { encoding: "utf8" },
  );
  if (res.status !== 0) return null;
  const sec = parseFloat(res.stdout.trim());
  return Number.isFinite(sec) ? sec : null;
}

// 集目录下的主片（形如 `第N集.mp4`），而非 video_source.mp4 等中间产物
function findMainVideo(epDir: string): string | null {
  return (
    readdirSync(epDir, { withFileTypes: true })
      .filter((d) => d.isFile() && d.name.endsWith(".mp4") && /第\d+集\.mp4$/.test(d.name))
      .map((d) => join(epDir, d.name))
      .shift() ?? null
  );
}

function epNumber(name: string): number {
  const m = name.match(/(\d+)/);
  return m ? parseInt(m[1], 10) : Number.MAX_SAFE_INTEGER;
}

function main() {
  const args = process.argv.slice(2);
  const seriesName = args.find((a) => !a.startsWith("--"));

  if (!seriesName) {
    console.log('用法: bun scripts/video-duration.ts "系列目录名"');
    const dirs = readdirSync(WORKFOLDER, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name);
    if (dirs.length) {
      console.log("可用的系列:");
      for (const dir of dirs) console.log(`  ${dir}`);
    }
    return;
  }

  const seriesDir = join(WORKFOLDER, seriesName);
  if (!existsSync(seriesDir)) {
    console.error(`系列目录不存在: ${seriesDir}`);
    return;
  }

  const episodeDirs = readdirSync(seriesDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort((a, b) => epNumber(a) - epNumber(b));

  const entries = [];
  for (const ep of episodeDirs) {
    const epDir = join(seriesDir, ep);
    const file = findMainVideo(epDir);
    if (!file) {
      console.log(`  ✗ ${ep}: 未找到主视频`);
      continue;
    }
    const sec = ffprobeDuration(file);
    if (sec === null) {
      console.log(`  ✗ ${ep}: ffprobe 读取时长失败`);
      continue;
    }
    const minutes = Math.floor(sec / 60);
    const seconds = Math.round(sec % 60);
    entries.push({
      episode: ep,
      file: file.replace(`${repoRoot}/`, ""),
      duration_seconds: sec,
      duration_fmt: `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`,
    });
    console.log(`  ✓ ${ep}: ${entries[entries.length - 1].duration_fmt} (${sec.toFixed(2)}s)`);
  }

  const totalSec = entries.reduce((sum, e) => sum + e.duration_seconds, 0);
  const totalMin = Math.floor(totalSec / 60);
  const totalS = Math.round(totalSec % 60);
  const total = {
    episodes: entries.length,
    duration_seconds: +totalSec.toFixed(2),
    duration_fmt: `${String(totalMin).padStart(2, "0")}:${String(totalS).padStart(2, "0")}`,
  };

  const outFile = join(seriesDir, "video-duration.json");
  writeFileSync(
    outFile,
    JSON.stringify({ series: seriesName, videos: entries, total }, null, 2) + "\n",
  );
  console.log(
    `\n输出: ${outFile} (${entries.length}/${episodeDirs.length} 集, 总计 ${total.duration_fmt})`,
  );
}

main();
