export interface WaveformPeaks {
  min: number;
  max: number;
}

export type PeaksPhase = "fetch" | "decode" | "analyze";

export interface PeaksProgress {
  phase: PeaksPhase;
  /** 0..1, decode 阶段无真实进度 (一次性阻塞), 仅在前后置 0/1 */
  ratio: number;
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
 * onProgress 提供 fetch(下载%)/decode/analyze 三个阶段的状态, 便于 UI 显示进度。
 */
export async function loadPeaks(
  url: string,
  buckets = 256,
  onProgress?: (p: PeaksProgress) => void,
): Promise<{ peaks: WaveformPeaks[]; durationMs: number }> {
  const hit = cache.get(url);
  if (hit) return hit;

  // 流式下载, 报真实字节进度
  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${url} failed: ${res.status}`);
  const total = Number(res.headers.get("content-length") ?? 0);
  if (!res.body) throw new Error(`fetch ${url} got no body`);
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (value) {
      chunks.push(value);
      received += value.length;
      if (total > 0) onProgress?.({ phase: "fetch", ratio: received / total });
    }
  }
  const audioBuf = new Uint8Array(received);
  let off = 0;
  for (const c of chunks) {
    audioBuf.set(c, off);
    off += c.length;
  }
  onProgress?.({ phase: "decode", ratio: 0 });

  const audio = await getAudioCtx().decodeAudioData(audioBuf.buffer.slice(0) as ArrayBuffer);

  onProgress?.({ phase: "analyze", ratio: 0 });
  const durationMs = audio.duration * 1000;

  const len = audio.length;
  const chCount = audio.numberOfChannels;
  const step = Math.max(1, Math.floor(len / buckets));
  const peaks: WaveformPeaks[] = [];
  // 按 step 分桶, 汇报粒度取每 32 桶, 避免大文件 UI 卡成假死
  const reportEvery = Math.max(1, Math.floor(buckets / 32));
  const totalReports = Math.max(1, Math.ceil(buckets / reportEvery));
  let reportIdx = 0;
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
    if (peaks.length % reportEvery === 0) {
      onProgress?.({ phase: "analyze", ratio: ++reportIdx / totalReports });
    }
  }
  onProgress?.({ phase: "analyze", ratio: 1 });

  const result = { peaks, durationMs };
  cache.set(url, result);
  return result;
}