import { For, Index, type Component } from "solid-js";
import type { Track } from "./consts";
import { AsrOcrFixTrack } from "./tracks/AsrOcrFixTrack";
import { AsrTrack } from "./tracks/AsrTrack";
import { MergeAudioTrack } from "./tracks/MergeAudioTrack";
import { SplitAudioTrack } from "./tracks/SplitAudioTrack";
import { TranslationTrack } from "./tracks/TranslationTrack";
import { TtsTrack } from "./tracks/TtsTrack";

interface Props {
  ref: (el: HTMLDivElement) => void;
  tracks: Track[];
  totalPx: number;
  pxPerMs: number;
  onSeek: (ms: number) => void;
  trackColor: (index: number, track: Track) => string;
  onScroll: () => void;
  taskDir?: string;
}

interface TrackComponentProps {
  track: Track;
  totalPx: number;
  pxPerMs: number;
  onSeek: (ms: number) => void;
  color: string;
  taskDir: string;
}

const trackComponents: Record<string, Component<TrackComponentProps>> = {
  asr: AsrTrack,
  asr_ocr_fix: AsrOcrFixTrack,
  sf_ocr_fix: AsrOcrFixTrack,
  split_audio: SplitAudioTrack,
  split_audio_timings: SplitAudioTrack,
  translation: TranslationTrack,
  tts: TtsTrack,
  merge_audio: MergeAudioTrack,
};

function DefaultTrack(props: TrackComponentProps) {
  const { track, pxPerMs, onSeek, color } = props;
  return (
    <div class="h-16 border-b relative">
      <Index each={track.segments}>
        {(seg) => (
          <div
            class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
            style={{
              left: `${seg().startMs * pxPerMs}px`,
              width: `${Math.max((seg().endMs - seg().startMs) * pxPerMs, 4)}px`,
              background: `${color}33`,
              "border-color": `${color}55`,
            }}
            onClick={() => onSeek(seg().startMs)}
            title={seg().text}
          >
            {seg().text}
          </div>
        )}
      </Index>
    </div>
  );
}

export function TimelineTracks(props: Props) {
  console.warn(
    `[TRACKS-ARR] len=${props.tracks.length} ids=${props.tracks.map((t) => t.id).join(",")} uniq=${new Set(props.tracks.map((t) => t.id)).size}`,
  );
  return (
    <div
      ref={(el) => {
        props.ref(el);
        console.warn(`[REF-TRACKS] set pid=${(el as any).__pid ?? "(none)"}`);
      }}
      class="flex-1 overflow-auto min-h-0"
      onScroll={props.onScroll}
    >
      <div class="relative" style={{ width: `${props.totalPx}px`, "min-width": "100%" }}>
        <For each={props.tracks}>
          {(track, i) => {
            const c = props.trackColor(i(), track);
            const Comp = trackComponents[track.id] || DefaultTrack;
            console.warn(
              `[TRACK] i=${i()} id=${track.id} rawColor=${track.color ?? "(none)"} resolved=${c} segs=${track.segments.length}`,
            );
            return (
              <Comp
                track={track}
                totalPx={props.totalPx}
                pxPerMs={props.pxPerMs}
                onSeek={props.onSeek}
                color={c}
                taskDir={props.taskDir ?? ""}
              />
            );
          }}
        </For>
      </div>
    </div>
  );
}
