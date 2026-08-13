import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { loadPeaks, type PeaksProgress, type WaveformPeaks } from "#/lib/audio/waveform";

interface Props {
  src: string;
  label?: string;
  onClose?: () => void;
}

function fmt(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const mmm = Math.floor(ms % 1000);
  return `${m}:${String(s % 60).padStart(2, "0")}.${String(mmm).padStart(3, "0")}`;
}

// canvas 不走 CSS 变量, 用临时元素让浏览器把 var(--x) (可能多层) 解析成具体颜色
const colorCache = new Map<string, string>();
function themeColor(name: string): string {
  const hit = colorCache.get(name);
  if (hit) return hit;
  const el = document.createElement("span");
  el.style.color = `var(${name})`;
  el.style.display = "none";
  document.body.appendChild(el);
  const color = getComputedStyle(el).color;
  el.remove();
  colorCache.set(name, color);
  return color;
}
const PLAYED = () => themeColor("--primary");
const UNPLAYED = () => themeColor("--muted-foreground");
const BAR_GAP = 1.5;

export function AudioPlayer(props: Props) {
  let audioRef!: HTMLAudioElement;
  let canvasRef!: HTMLCanvasElement;
  const [playing, setPlaying] = createSignal(false);
  const [current, setCurrent] = createSignal(0);
  const [duration, setDuration] = createSignal(0);
  const [peaks, setPeaks] = createSignal<WaveformPeaks[] | null>(null);
  const [loadErr, setLoadErr] = createSignal<string | null>(null);
  const [progress, setProgress] = createSignal<PeaksProgress | null>(null);

  let dragging = false;

  const toggle = () => {
    if (playing()) audioRef.pause();
    else audioRef.play().catch(() => {});
  };

  const seekAt = (clientX: number) => {
    const rect = canvasRef.getBoundingClientRect();
    if (!rect.width) return;
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    audioRef.currentTime = ratio * audioRef.duration;
    setCurrent(audioRef.currentTime * 1000);
  };

  const draw = () => {
    const canvas = canvasRef;
    const dpr = window.devicePixelRatio || 1;
    const width = Math.max(1, canvas.clientWidth);
    const height = Math.max(1, canvas.clientHeight);
    if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
    }
    const g = canvas.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, height);

    const durMs = duration();
    const curMs = current();

    const progressLabel = (p: PeaksProgress): string => {
      const pct = Math.round(p.ratio * 100);
      if (p.phase === "fetch") return pct >= 100 ? "解码中…" : `下载 ${pct}%`;
      if (p.phase === "decode") return "解码中…";
      return `波形 ${pct}%`;
    };

    // 波形未就绪: 画纯进度条占位 + 加载进度文字 (仍可点击/拖动 seek)
    const data = peaks();
    if (!data || !durMs) {
      g.fillStyle = UNPLAYED();
      g.fillRect(0, height / 2 - 1, width, 2);
      g.fillStyle = PLAYED();
      g.fillRect(0, height / 2 - 1, Math.min(width, (width * curMs) / durMs || 0), 2);

      const p = progress();
      const label = loadErr() ? "波形加载失败" : p ? progressLabel(p) : "波形加载中…";
      g.fillStyle = loadErr() ? "oklch(0.6 0.2 25)" : UNPLAYED();
      g.font = "10px ui-monospace, monospace";
      g.textAlign = "center";
      g.textBaseline = "middle";
      g.fillText(label, width / 2, height / 2 - 8);
      return;
    }

    const n = data.length;
    const barW = Math.max(0.5, width / n - BAR_GAP);
    const mid = height / 2;
    const playedColor = PLAYED();
    const unplayedColor = UNPLAYED();
    for (let i = 0; i < n; i++) {
      const { min, max } = data[i];
      const top = Math.max(0, (1 - max) * mid);
      const bot = Math.min(height, (1 - min) * mid + mid);
      const h = Math.max(1, bot - top);
      g.fillStyle = (i / n) * durMs <= curMs ? playedColor : unplayedColor;
      g.fillRect((i * width) / n, top, Math.max(0.5, barW), h);
    }
  };

  // 进度/时长/波形/加载状态任一变化即重绘
  createEffect(() => {
    current();
    duration();
    peaks();
    progress();
    loadErr();
    draw();
  });

  onMount(() => {
    audioRef.addEventListener("loadedmetadata", () =>
      setDuration(Math.floor(audioRef.duration * 1000)),
    );
    audioRef.addEventListener("timeupdate", () => setCurrent(Math.floor(audioRef.currentTime * 1000)));
    audioRef.addEventListener("ended", () => setPlaying(false));
    audioRef.addEventListener("play", () => setPlaying(true));
    audioRef.addEventListener("pause", () => setPlaying(false));

    loadPeaks(props.src, 256, setProgress)
      .then(({ peaks: p }) => setPeaks(p))
      .catch((e: unknown) => {
        setPeaks(null);
        setLoadErr(String(e instanceof Error ? e.message : e));
      });

    const ro = new ResizeObserver(() => draw());
    ro.observe(canvasRef);
    onCleanup(() => ro.disconnect());
  });

  onCleanup(() => {
    audioRef.pause();
    audioRef.src = "";
  });

  return (
    <div class="flex items-center gap-2 rounded-lg border bg-background px-3 py-2 text-sm shadow-sm min-w-80">
      <audio ref={audioRef!} src={props.src} preload="auto" />

      <button
        class="flex size-7 items-center justify-center rounded-full bg-primary text-primary-foreground text-xs cursor-pointer shrink-0"
        onClick={toggle}
      >
        {playing() ? "⏸" : "▶"}
      </button>

      <canvas
        ref={canvasRef}
        class="h-12 flex-1 cursor-pointer"
        onPointerDown={(e) => {
          dragging = true;
          e.currentTarget.setPointerCapture(e.pointerId);
          seekAt(e.clientX);
        }}
        onPointerMove={(e) => dragging && seekAt(e.clientX)}
        onPointerUp={() => (dragging = false)}
        onPointerCancel={() => (dragging = false)}
      />

      <span class="text-xs text-muted-foreground whitespace-nowrap tabular-nums">
        {fmt(current())} / {fmt(duration())}
      </span>

      {props.label && (
        <span class="text-xs text-muted-foreground truncate max-w-24">{props.label}</span>
      )}

      {props.onClose && (
        <button
          class="ml-1 text-muted-foreground hover:text-foreground text-xs cursor-pointer shrink-0"
          onClick={props.onClose}
        >
          ✕
        </button>
      )}
    </div>
  );
}