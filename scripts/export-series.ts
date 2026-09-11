#!/usr/bin/env bun
import { join, resolve } from "node:path";
import { existsSync, readdirSync, copyFileSync, mkdirSync, readFileSync } from "node:fs";

const repoRoot = resolve(import.meta.dir, "..");
const WORKFOLDER = process.env["WORKFOLDER"]
  ? resolve(repoRoot, process.env["WORKFOLDER"])
  : join(repoRoot, "workfolder");

// 优先读 Rust 写回 ctx.json 的 final_video_path（权威成片路径），
// 再按目录布局回退候选。当前 Rust 管线输出到 mix_video/，旧 TS/更老布局为
// merge_video/ / media/，final_video_dir() 见
// (packages/core/src/steps/utils/mod.rs)：
// 1. <集>/ctx.json  final_video_path
// 2. dub + sf_ocr:           <集>/mix_video/dub_sf_ocr/<集>.mp4 (首选, 关键帧 OCR 最优字幕源)
// 3. dub + asr_ocr:          <集>/mix_video/dub_asr_ocr/<集>.mp4
// 4. dub + asr (默认):       <集>/mix_video/dub/<集>.mp4
// 5. 无翻译变体:             <集>/mix_video/dub_sf_ocr_ntl|dub_asr_ocr_ntl/<集>.mp4
// 6. 旧 merge_video 同构布局 (TS 时代)
// 7. 更老的 media:           <集>/media/<集>_dub_asr_ocr.mp4
function findDub(seriesDir: string, ep: string): string | null {
  const epDir = join(seriesDir, ep);
  const fromCtx = (() => {
    try {
      const ctx = JSON.parse(readFileSync(join(epDir, "ctx.json"), "utf8")) as {
        task?: { final_video_path?: string };
      };
      return ctx.task?.final_video_path;
    } catch {
      return undefined;
    }
  })();
  if (fromCtx && existsSync(fromCtx)) return fromCtx;

  const variants = [
    ["mix_video", "dub_sf_ocr"],
    ["mix_video", "dub_asr_ocr"],
    ["mix_video", "dub"],
    ["mix_video", "dub_sf_ocr_ntl"],
    ["mix_video", "dub_asr_ocr_ntl"],
    ["merge_video", "dub_sf_ocr"],
    ["merge_video", "dub_asr_ocr"],
    ["merge_video", "dub"],
    ["merge_video", "dub_sf_ocr_ntl"],
    ["merge_video", "dub_asr_ocr_ntl"],
  ];
  const candidates = [
    ...variants.map(([dir, sub]) => join(epDir, dir, sub, `${ep}.mp4`)),
    join(epDir, "merge_video", `${ep}_dub_asr_ocr.mp4`),
    join(epDir, "media", `${ep}_dub_asr_ocr.mp4`),
  ];
  return candidates.find((p) => existsSync(p)) ?? null;
}

// 从集目录名提取集号用于排序：`01`/`8`/`第10集` → 1/8/10
function epNumber(name: string): number {
  const m = name.match(/(\d+)/);
  return m ? parseInt(m[1], 10) : Number.MAX_SAFE_INTEGER;
}

function listSeries(): string[] {
  return readdirSync(WORKFOLDER, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort();
}

function exportSeries(seriesName: string, force: boolean): void {
  const seriesDir = join(WORKFOLDER, seriesName);
  if (!existsSync(seriesDir)) {
    console.error(`系列目录不存在: ${seriesName}`);
    return;
  }

  const episodeDirs = readdirSync(seriesDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort((a, b) => epNumber(a) - epNumber(b));

  if (episodeDirs.length === 0) {
    console.log(`"${seriesName}" 下没有剧集目录`);
    return;
  }

  const outDir = join(repoRoot, "out", seriesName);
  mkdirSync(outDir, { recursive: true });

  let copied = 0,
    skipped = 0,
    failed = 0;

  for (const ep of episodeDirs) {
    const src = findDub(seriesDir, ep);
    const dst = join(outDir, `${ep}.mp4`);

    if (!src) {
      console.log(`  ✗ ${ep}: 源文件缺失`);
      failed++;
      continue;
    }

    if (existsSync(dst) && !force) {
      console.log(`  · ${ep}: 已存在, 跳过`);
      skipped++;
      continue;
    }

    copyFileSync(src, dst);
    console.log(`  ✓ ${ep} → out/${seriesName}/${ep}.mp4`);
    copied++;
  }

  console.log(`\n${seriesName}: ${copied} 复制, ${skipped} 跳过, ${failed} 失败`);
}

function main() {
  const args = process.argv.slice(2);
  const force = args.includes("--force");
  const all = args.includes("--all");
  const seriesName = args.find((a) => !a.startsWith("--"));

  if (!all && !seriesName) {
    const dirs = listSeries();
    if (dirs.length === 0) {
      console.log(`${WORKFOLDER}/ 下没有系列目录`);
      return;
    }
    console.log("可导出的系列:");
    for (const dir of dirs) {
      const episodes = readdirSync(join(WORKFOLDER, dir), { withFileTypes: true })
        .filter((d) => d.isDirectory())
        .map((d) => d.name);
      console.log(`  ${dir} (${episodes.length} 集)`);
    }
    return;
  }

  if (all) {
    for (const dir of listSeries()) {
      exportSeries(dir, force);
    }
    return;
  }

  exportSeries(seriesName as string, force);
}

main();
