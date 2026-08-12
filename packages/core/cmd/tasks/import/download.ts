import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { emitLog, ffmpeg, nowISO } from "@repo/core/stages/utils/utils.ts";
import {
  TaskCtx,
  VideoSource,
  readCtx,
  setCtx,
  setStage,
  setTask,
  writeCtx,
} from "@repo/core/context/context.ts";
import { startLog } from "../../../stages/utils/log.ts";
import { getStages } from "@repo/core/stages/utils/stages";
import { InputArgs } from "@repo/core/input/input";
import {
  autoGroupIdAndVideoId,
  copyFileToPath,
  downloadRemoteVideo,
  encodeToMp4,
} from "./utils.ts";
import { WORKFOLDER } from "@repo/config/path/paths";
import { probeFrameRate } from "../../../utils/ffmpeg.ts";
import { log } from "@repo/util/log";

export const importVideo = async (input: InputArgs) => {
  const args = input.task ?? {};
  const url = args.url;
  if (!url) {
    console.error("task start: need task.url in input.json");
    process.exit(1);
  }
  const { groupId, taskId, ytDlpExtArgs, title, source } = await autoGroupIdAndVideoId(url);
  startLog("import", taskId);
  const taskDir = join(WORKFOLDER, groupId, taskId);
  mkdirSync(taskDir, { recursive: true });
  const { downloadedVideoPath, videoPath, audioPath } = await downloadVideo(
    url,
    source,
    groupId,
    taskId,
    ytDlpExtArgs,
  );
  // ✅ 探测视频帧率并写回 context
  const frame_rate = probeFrameRate(videoPath);
  emitLog(
    taskDir,
    `[Download] Video frame rate: ${frame_rate.numerator}/${frame_rate.denominator}`,
  );

  const stages = getStages(input.task.pipeline);
  const ctx: TaskCtx = {
    task: {
      id: taskId,
      status: "queued",
      source: source,
      url,
      created_at: nowISO(),
      task_dir: taskDir,
      title: title || taskId,
    },
    frame_rate,
    asr_language: input.task.sourceLang || "auto",
    pipeline: input.task.pipeline || "dub",
    lastRunPipeline: input.task.pipeline || "dub",
    input,
    stages: stages.map((stage) => ({
      task_id: taskId,
      name: stage,
      label: stage,
      status: "pending",
    })),
    video_source_path: videoPath,
    audioSourcePath: audioPath,
  };

  writeCtx(ctx);
  return ctx;
};

export async function downloadVideo(
  url: string,
  source: VideoSource,
  groupId: string,
  taskId: string,
  ytDlpExtArgs: string[],
) {
  const taskDir = join(WORKFOLDER, groupId, taskId);
  let downloadedVideoPath = join(taskDir, `${taskId}.mp4`);
  const videoPath = join(taskDir, "video_source.mp4");
  // Extract audio for downstream stages
  const audioPath = join(taskDir, "audio_source.wav");
  if (source === "local" || source === "remote") {
    if (source === "local") {
      copyFileToPath(url, downloadedVideoPath);
    } else if (source === "remote") {
      downloadedVideoPath = await downloadRemoteVideo(url, taskDir);
    }

    emitLog(taskDir, "[Download] Importing local video...");

    const t0 = Date.now();
    encodeToMp4(downloadedVideoPath, videoPath);

    const elapsedSec = (Date.now() - t0) / 1000;

    if (!existsSync(videoPath)) throw new Error("ffmpeg did not produce video_source.mp4");

    const sizeMb = (statSync(videoPath).size / 1024 / 1024).toFixed(1);
    emitLog(taskDir, `[Download] Imported in ${elapsedSec.toFixed(1)}s (${sizeMb}MB)`);

    emitLog(taskDir, "[Download] Extracting audio_source.wav...");
    ffmpeg(["-i", videoPath, "-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2", audioPath]);
  } else if (source === "youtube" || source === "bilibili") {
    // console.log(`[Download] video info: `, info);

    emitLog(taskDir, "[Download] Downloading video...");

    ytDlpExtArgs.push("--remote-components", "ejs:github");
    const ytArgs: string[] = [
      "-f",
      "bestaudio[ext=m4a]+bestvideo[ext=mp4]/best[ext=mp4]/best",
      "--merge-output-format",
      "mp4",
      "-o",
      join(taskDir, "video_source.%(ext)s"),
      ...ytDlpExtArgs,
      url,
    ];

    const t0 = Date.now();
    const r = spawnSync("yt-dlp", ytArgs, {
      stdio: ["pipe", "pipe", "pipe"],
      timeout: 300_000,
    });
    const elapsedSec = (Date.now() - t0) / 1000;

    const dlErr = r.error;
    if (dlErr) throw new Error(`yt-dlp: ${dlErr.message}`);
    if (r.status !== 0) {
      const stderr = r.stderr.toString();

      console.warn(`[Download] yt-dlp failed:`, stderr);
      if (/^ERROR:/im.test(stderr)) {
        throw new Error(`yt-dlp exit ${r.status}: ${stderr}`);
      }
    }

    if (!existsSync(videoPath)) throw new Error("yt-dlp did not produce video_source.mp4");

    const sizeMb = (statSync(videoPath).size / 1024 / 1024).toFixed(1);
    emitLog(taskDir, `[Download] Downloaded in ${elapsedSec.toFixed(1)}s (${sizeMb}MB)`);
    emitLog(taskDir, `[Download] Speed ${(Number(sizeMb) / elapsedSec).toFixed(2)} MB/s`);

    // Extract audio for downstream stages
    log("Extracting audio_source.wav...");
    ffmpeg(["-i", videoPath, "-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2", audioPath]);
  }

  return { downloadedVideoPath, videoPath, audioPath };
}
