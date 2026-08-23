import { mkdirSync } from "node:fs";
import { env } from "./env.ts";
import { DATA_DIR, COOKIE_DIR, LOG_DIR } from "./path/paths.ts";

export function device(): string {
  return env.CUDA_DEVICE ?? env.DEVICE;
}

export interface OpenAIDefaults {
  baseUrl: string;
  apiKey: string;
  model: string;
  translateConcurrency: number;
}

export function openaiDefaults(): OpenAIDefaults {
  return {
    baseUrl: env.OPENAI_BASE_URL,
    apiKey: env.OPENAI_API_KEY,
    model: env.OPENAI_MODEL,
    translateConcurrency: env.OPENAI_TRANSLATE_CONCURRENCY,
  };
}

export function ffmpegBinary(): string {
  return env.FFMPEG_PATH;
}

export function ffprobeBinary(): string {
  return env.FFPROBE_PATH;
}

export interface YtDlpDefaults {
  proxyPort?: string;
}

export function ytdlpDefaults(): YtDlpDefaults {
  return {
    proxyPort: env.YTDLP_PROXY_PORT,
  };
}

export function ensureRuntimeDirs(): void {
  for (const dir of [DATA_DIR, COOKIE_DIR, LOG_DIR, env.WORKFOLDER, env.MODEL_CACHE_DIR]) {
    mkdirSync(dir, { recursive: true });
  }
}
