import { $, spawn } from "bun";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { REPO_ROOT } from "@repo/config/root";
import { log } from "@repo/util/log";
import { OcrSegmentFilterResult } from "@repo/subtitle-ocr/ocr_fix/segment_filter";
import { readJson } from "../../../utils/fileOps";
import { OcrFramesResult } from "@repo/subtitle-ocr/types";
import { OcrFixArgs } from "../fix_args";

export const ensureOcrBin = async () => {
  const ocrBin = join(REPO_ROOT, "target", "release", "subtitle-ocr");
  if (!existsSync(ocrBin)) {
    log(`[sf_ocr] subtitle-ocr 未构建，自动编译...`);
    const build = await $`cargo build --release -p subtitle-ocr-cli --bin subtitle-ocr`
      .cwd(REPO_ROOT)
      .nothrow();
    if (build.exitCode !== 0) {
      throw new Error(`subtitle-ocr 编译失败 (exit ${build.exitCode}):\n${build.stderr}`);
    }
  }
  return ocrBin;
};

export const cellOcr = async (
  preDir: string,
  outFile: string,
  ocrArgs: {
    text_confidence_threshold: number;
    subtitleOnly: boolean;
  },
) => {
  const ocrBin = await ensureOcrBin();
  const args = [
    "--dir",
    preDir,
    "--out",
    outFile,
    "--text-confidence-threshold",
    String(ocrArgs.text_confidence_threshold),
    ...(ocrArgs.subtitleOnly ? ["--subtitle-only"] : []),
  ];
  log(`subtitle-ocr ${args.join(" ")}`);
  const proc = spawn([ocrBin, ...args], {
    cwd: REPO_ROOT,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    throw new Error(`subtitle-ocr failed with exit code ${exitCode}`);
  }

  const data = JSON.parse(readFileSync(outFile, "utf-8")) as OcrFramesResult;
  const frameResults = data.frames ?? [];
  if (!frameResults.length) {
    throw new Error("sf_ocr: no OCR results from keyframes");
  }
  log(`${frameResults.length} frame results -> ${outFile}`);
  return data;
};

export const ensureOcrPostBin = async () => {
  const ocrPostBin = join(REPO_ROOT, "target", "release", "ocr-post");
  if (!existsSync(ocrPostBin)) {
    log(`ocr-post 未构建，自动编译...`);
    const build = await $`cargo build --release -p subtitle-ocr-cli --bin ocr-post`
      .cwd(REPO_ROOT)
      .nothrow();
    if (build.exitCode !== 0) {
      throw new Error(`ocr-post 编译失败 (exit ${build.exitCode}):\n${build.stderr}`);
    }
  }
  return ocrPostBin;
};

export const cellOcrPost = async (
  framesFile: string,
  videoFile: string,
  outDir: string,
  args: OcrFixArgs,
) => {
  const ocrPostBin = await ensureOcrPostBin();

  const postArgs = [
    "--frames",
    framesFile,
    "--video",
    videoFile,
    "--out",
    outDir,
    "--threshold",
    String(args.adjusted_confidence_threshold),
    "--stop-at",
    "filter-segment",
  ];
  log(`ocr-post ${postArgs.join(" ")}`);
  const proc = spawn([ocrPostBin, ...postArgs], {
    cwd: REPO_ROOT,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    throw new Error(`ocr-post failed with exit code ${exitCode}`);
  }
  const filterFile = join(outDir, "segment_filter.json");
  const filtered = await readJson<OcrSegmentFilterResult>(filterFile);
  return filtered;
};
