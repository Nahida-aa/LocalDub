import { resolve } from "node:path";
import { config as loadEnv } from "dotenv";
import { config_dir, resolve_path } from "./root.ts";

loadEnv({ path: resolve(config_dir(), ".env") });

function envStr(key: string, fallback: string): string {
  return process.env[key]?.trim() || fallback;
}

function envStrUndefined(key: string): string | undefined {
  const v = process.env[key]?.trim();
  return v || undefined;
}

export const workfolder = () => resolve_path(envStr("WORKFOLDER", "workfolder"));

export const env = {
  // DB
  // DB_FILE_NAME: resolve(REPO_ROOT, process.env.DB_FILE_NAME ?? 'data/youdub.sqlite'),

  // Paths
  WORKFOLDER: workfolder(),
  MODEL_CACHE_DIR: resolve_path(envStr("MODEL_CACHE_DIR", "data/models")),

  // Device
  DEVICE: envStr("DEVICE", "auto"),
  CUDA_DEVICE: envStrUndefined("CUDA_DEVICE"),

  // Translate API
  OPENAI_BASE_URL: envStr("OPENAI_BASE_URL", "http://localhost:11434/v1"),
  OPENAI_API_KEY: envStr("OPENAI_API_KEY", "sk-xxxx"),
  OPENAI_MODEL: envStr("OPENAI_MODEL", "gemma4:31b-cloud"),
  OPENAI_TRANSLATE_CONCURRENCY: parseInt(envStr("OPENAI_TRANSLATE_CONCURRENCY", "50"), 10),

  // yt-dlp
  YTDLP_PROXY_PORT: envStrUndefined("YTDLP_PROXY_PORT"),

  // FFmpeg
  FFMPEG_PATH: envStr("FFMPEG_PATH", "ffmpeg"),
  FFPROBE_PATH: envStr("FFPROBE_PATH", "ffprobe"),

  // CORS
  CORS_ALLOW_ORIGINS: envStrUndefined("CORS_ALLOW_ORIGINS"),
  CORS_ALLOW_ORIGIN_REGEX: envStrUndefined("CORS_ALLOW_ORIGIN_REGEX"),
} as const;
