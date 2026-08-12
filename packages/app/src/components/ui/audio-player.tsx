import { createSignal, onCleanup, onMount } from "solid-js";

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

export function AudioPlayer(props: Props) {
  let audioRef!: HTMLAudioElement;
  const [playing, setPlaying] = createSignal(false);
  const [current, setCurrent] = createSignal(0);
  const [duration, setDuration] = createSignal(0);

  onMount(() => {
    audioRef.addEventListener("loadedmetadata", () => setDuration(Math.floor(audioRef.duration * 1000)));
    audioRef.addEventListener("timeupdate", () => setCurrent(Math.floor(audioRef.currentTime * 1000)));
    audioRef.addEventListener("ended", () => setPlaying(false));
    audioRef.addEventListener("play", () => setPlaying(true));
    audioRef.addEventListener("pause", () => setPlaying(false));
  });

  onCleanup(() => {
    audioRef.pause();
    audioRef.src = "";
  });

  const toggle = () => {
    if (playing()) { audioRef.pause(); }
    else { audioRef.play().catch(() => {}); }
  };

  const seek = (e: Event) => {
    const pct = parseFloat((e.currentTarget as HTMLInputElement).value);
    audioRef.currentTime = (pct / 100) * audioRef.duration;
  };

  const pct = () => (duration() > 0 ? (current() / duration()) * 100 : 0);

  return (
    <div class="flex items-center gap-2 rounded-lg border bg-background px-3 py-2 text-sm shadow-sm min-w-64">
      <audio ref={audioRef!} src={props.src} preload="auto" />

      <button
        class="flex size-7 items-center justify-center rounded-full bg-primary text-primary-foreground text-xs cursor-pointer shrink-0"
        onClick={toggle}
      >
        {playing() ? "⏸" : "▶"}
      </button>

      <input
        class="flex-1 h-1 accent-primary cursor-pointer"
        type="range"
        min={0}
        max={100}
        step={0.1}
        value={pct()}
        onInput={seek}
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
