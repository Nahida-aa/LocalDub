import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { env } from "@repo/config/env";
import { classifySource, extractVideoId, isYouTubeUrl } from "../../../utils/validate.ts";
import { emitLog, ffmpeg, nowISO } from "@repo/core/stages/utils/utils.ts";
import { to } from "@repo/shared/lib/utils/try.ts";
import { startLog } from "../../../stages/utils/log.ts";
import { copyFileSync } from "@repo/core/utils/fileOps";
import { WORKFOLDER, YOUTUBE_COOKIE_PATH } from "@repo/config/path/paths";
import { sanitizeText } from "@repo/core/utils/utils";
import { writeJson } from "@repo/util/file_op";

function parseDirAndId(filePath: string): { dir: string; id: string } {
  const parts = filePath.split(/[/\\]/).filter(Boolean);
  if (parts.length === 0) return { dir: "default", id: "video" };
  const fileName = parts.pop() || "video";
  const id = fileName.replace(/\.[^.]+$/, "");
  const parentDir = parts.pop() || "default";
  const genericDirs = new Set([
    "videos",
    "video",
    "downloads",
    "download",
    "media",
    "tmp",
    "temp",
    "uploads",
    "upload",
    "files",
    "workfolder",
    "local",
  ]);
  if (genericDirs.has(parentDir.toLowerCase()) && parts.length > 0) {
    const grandParentDir = parts.pop();
    if (grandParentDir && !genericDirs.has(grandParentDir.toLowerCase())) {
      return { dir: grandParentDir, id };
    }
  }
  return { dir: parentDir || "root", id };
}

export const copyFileToPath = (src: string, target: string) => copyFileSync(src, target);
export const autoGroupIdAndVideoId = async (url: string) => {
  const source = await classifySource(url);
  const ret = {
    groupId: "root",
    taskId: "unset",
    source,
    ytDlpExtArgs: [] as string[],
    title: undefined as string | undefined,
  };
  if (source === "local" || source === "remote") {
    const { dir: groupId, id: taskId } = parseDirAndId(url);
    ret.groupId = groupId;
    ret.taskId = taskId;
  } else if (source === "youtube" || source === "bilibili") {
    // YouTube/Bilibili URL — download via yt-dlp
    const isYT = source === "youtube";
    const ytDlpExtArgs: string[] = [];
    if (isYT && existsSync(YOUTUBE_COOKIE_PATH)) {
      ytDlpExtArgs.push("--cookies", YOUTUBE_COOKIE_PATH);
    }
    if (isYT && env.YTDLP_PROXY_PORT) {
      ytDlpExtArgs.push("--proxy", `http://127.0.0.1:${env.YTDLP_PROXY_PORT}`);
    }
    let index: string | undefined = undefined;
    try {
      const urlObj = new URL(url);
      if (urlObj.searchParams.has("list")) {
        index = urlObj.searchParams.get("index") || "1";
        ytDlpExtArgs.push("--playlist-items", index);
      } else {
        ytDlpExtArgs.push("--no-playlist");
      }
    } catch {
      ytDlpExtArgs.push("--no-playlist");
    }
    const infoArgs = ["--dump-json", ...ytDlpExtArgs, url];
    console.log(`[autoGroupIdAndVideoId] yt-dlp`, infoArgs);
    const infoR = spawnSync("yt-dlp", infoArgs, {
      stdio: ["pipe", "pipe", "pipe"],
      timeout: 30_000,
    });
    ret.ytDlpExtArgs = ytDlpExtArgs;
    if (infoR.status !== 0) {
      const stderr = infoR.stderr.toString();
      console.warn(`[autoGroupIdAndVideoId] yt-dlp failed:`, stderr);
      if (/Sign in to confirm|no longer valid/i.test(stderr)) {
        throw new Error(
          `YouTube cookie expired or invalid.\n` +
            `Export fresh cookies with your browser, then run:\n` +
            `  command: cookie\n` +
            `in input.json, with the Netscape cookie content.`,
        );
      } else if (/^ERROR:/im.test(stderr)) {
        throw new Error(`yt-dlp exit`);
      }
    }
    if (infoR.stdout.length > 0) {
      const info = JSON.parse(infoR.stdout.toString());
      // console.log(`[autoGroupIdAndVideoId] yt-dlp info:`, info);
      const groupName =
        info.playlist_title && info.uploader
          ? sanitizeText(`${info.uploader}-${info.playlist_title}`)
          : info.playlist_title
            ? sanitizeText(info.playlist_title)
            : sanitizeText(info.uploader ?? "unknown");
      const videoId: string = info.id || extractVideoId(url);
      ret.groupId = groupName;
      ret.taskId = index ?? videoId;
      const taskDir = join(WORKFOLDER, ret.groupId, ret.taskId);

      if (info) {
        mkdirSync(taskDir, { recursive: true });
        const [_, err] = to(() => writeJson(join(taskDir, "ytdlp_info.json"), info));
        if (err) {
          console.warn("[Download] Failed to write ytdlp_info.json", err);
        }
      }
    } else {
      ret.taskId = extractVideoId(url);
    }
  }
  return ret;
};
export const downloadRemoteVideo = async (url: string, taskDir: string) => {
  const urlO = new URL(url);
  const filename = basename(urlO.pathname) || "video.mp4";
  const rawVideoPath = join(taskDir, filename);
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`Download failed: ${resp.status} ${resp.statusText}`);
  const buf = Buffer.from(await resp.arrayBuffer());
  writeFileSync(rawVideoPath, buf);
  return rawVideoPath;
};
/**
 * 重新编码视频为 H.264 + AAC 格式的
MP4
 */
export const encodeToMp4 = (inputPath: string, outputPath: string) =>
  ffmpeg([
    "-i",
    inputPath,
    "-map",
    "0:v:0", // 选择第一个视频流
    "-map",
    "0:a:0?", // 选择第一个音频流（如果存在）
    "-c:v",
    "libx264", // 视频编码器 H.264
    "-preset",
    "fast", // 编码速度/压缩比平衡: fast
    "-crf",
    "23", // 视频质量控制参数，范围 0-51，越小质量越好，文件越大
    "-c:a",
    "aac", // 音频编码器 AAC
    "-movflags",
    "+faststart", // 允许视频在未完全下载时播放
    outputPath,
  ]);
