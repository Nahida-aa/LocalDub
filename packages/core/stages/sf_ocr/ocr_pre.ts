import { $, spawn } from "bun";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { ensureDir } from "@repo/util/file_op";
import { emitLog, nowISO, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { REPO_ROOT } from "@repo/config/root";
import { log } from "@repo/util/log";

// 关键帧策略前处理：调 sf-cli（subtitle-finder 封装）找字幕关键帧。
// 落盘 `<taskDir>/sf_ocr_pre/`：frames/（原始关键帧 PNG）、mask/（去背景掩码）、
// timeline.txt、keyframes.json。OCR 识别是下游 sf_ocr stage 的事。
export async function stageSfOcrPre(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  startLog(ctx.task.current_stage, ctx.task.id);
  setStage(taskDir, "sf_ocr_pre", {
    last_message: "查找字幕关键帧...",
    progress: 0,
  });

  const videoPath = video_source_path(ctx);
  if (!existsSync(videoPath)) {
    throw new Error(`OCR input not found: ${videoPath}`);
  }

  const sfBin = join(REPO_ROOT, "target", "release", "sf-cli");
  if (!existsSync(sfBin)) {
    log(`[sf_ocr_pre] sf-cli 未构建，自动编译...`);
    const build = await $`cargo build --release -p sf-cli --bin sf-cli`.cwd(REPO_ROOT).nothrow();
    if (build.exitCode !== 0) {
      throw new Error(`sf-cli 编译失败 (exit ${build.exitCode}):\n${build.stderr}`);
    }
  }

  const outDir = resolve(taskDir, "sf_ocr_pre");

  log(`sf-cli ${videoPath} --out ${outDir}`);
  const proc = spawn([sfBin, videoPath, "--out", outDir], {
    cwd: REPO_ROOT,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    throw new Error(`sf-cli failed with exit code ${exitCode}`);
  }

  const frameDir = join(outDir, "frames");
  if (!existsSync(frameDir)) {
    throw new Error(`sf-cli 未产出关键帧目录: ${frameDir}`);
  }
  const kfJson = join(outDir, "keyframes.json");
  const keyframes = existsSync(kfJson) ? JSON.parse(readFileSync(kfJson, "utf-8")) : [];
  log(`[sf_ocr_pre] ${keyframes.length} keyframes -> ${outDir}`);

  await setStage(taskDir, "sf_ocr_pre", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: `找到 ${keyframes.length} 个关键帧`,
  });
}
