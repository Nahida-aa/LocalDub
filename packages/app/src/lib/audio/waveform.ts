export interface WaveformPeaks {
  min: number;
  max: number;
}

const cache = new Map<string, { peaks: WaveformPeaks[]; durationMs: number }>();

let ctx: AudioContext | null = null;
function getAudioCtx(): AudioContext {
  // 解码不依赖 resume (decodeAudioData 与 autoplay policy 无关), 单例复用即可
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

/**
 * 拉取音频并解码, 下采样成固定数量 bucket 的 min/max 峰值 (每 bucket 一列 bar)。
 * 按 url 缓存, 组件卸载/重挂载间复用, 避免重复解码。
 */
export async function loadPeaks(
  url: string,
  buckets = 256,
): Promise<{ peaks: WaveformPeaks[]; durationMs: number }> {
  const hit = cache.get(url);
  if (hit) return hit;

  const res = await fetch(url);
  const audioBuf = await res.arrayBuffer();
  const audio = await getAudioCtx().decodeAudioData(audioBuf);
  const durationMs = audio.duration * 1000;

  const len = audio.length;
  const chCount = audio.numberOfChannels;
  const step = Math.max(1, Math.floor(len / buckets));
  const peaks: WaveformPeaks[] = [];
  for (let i = 0; i < len; i += step) {
    const iEnd = Math.min(len, i + step);
    let min = 1;
    let max = -1;
    for (let ch = 0; ch < chCount; ch++) {
      const data = audio.getChannelData(ch);
      for (let j = i; j < iEnd; j++) {
        const v = data[j];
        if (v < min) min = v;
        if (v > max) max = v;
      }
    }
    peaks.push({ min, max });
  }

  const result = { peaks, durationMs };
  cache.set(url, result);
  return result;
}