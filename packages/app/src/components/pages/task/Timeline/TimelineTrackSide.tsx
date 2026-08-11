import { For, Show } from "solid-js";
import type { Track } from "./consts";
import { useTrackMetaRecord } from "./tracks/presence";

interface Props {
  ref: (el: HTMLDivElement) => void;
  tracks: Track[];
  trackColor: (index: number, track: Track) => string;
  taskDir: string;
}

export function TimelineTrackSide(props: Props) {
  const meta = useTrackMetaRecord();
  return (
    <div class="w-30 shrink-0 border-r flex flex-col">
      <div class="border-b bg-muted/20 shrink-0">
        <div class="h-5" />
      </div>
      <div ref={props.ref} class="flex-1 overflow-hidden">
        <For each={props.tracks}>
          {(track, i) => {
            const m = meta()[`${props.taskDir}/${track.id}`];
            const show = () => !!m?.present;
            const label = () => m?.label ?? track.label;
            return (
              <Show when={show()}>
                <div
                  class="h-16 border-b flex items-center px-3 text-xs text-muted-foreground truncate"
                  style={{ "border-left": `3px solid ${props.trackColor(i(), track)}` }}
                >
                  {label()}
                </div>
              </Show>
            );
          }}
        </For>
      </div>
    </div>
  );
}
