import { ffmpeg } from "@repo/core/stages/utils/utils.ts";

/**
 * 从源音频流拷贝 [startMs, endMs) 范围到 outPath, 不重编码。
 *
 * 用 `-c copy` 流拷贝, 需源为可 seek 的 wav/pcm (快)。
 * 位置参数取毫秒, 内部转秒交给 ffmpeg。
 */
export function cutAudioRange(source: string, startMs: number, endMs: number, outPath: string) {
  ffmpeg([
    "-i",
    source,
    "-ss",
    String(startMs / 1000),
    "-to",
    String(endMs / 1000),
    "-c",
    "copy",
    outPath,
  ]);
}
