import { FastForward, Rewind, Play, Pause } from "lucide-solid";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@repo/ui-solid/base/select";
import { msToTimecodeFull } from "../../../../../../util/timecode";
import { EditableTimecode } from "#/components/ui/editable-timecode";
import {
  useCurrentTime,
  useDuration,
  useFps,
  usePlaying,
  usePlaybackRate,
} from "../store/videoViewer";

export interface VideoControlsProps {
  onTogglePlay: () => void;
  onRateChange: (rate: number) => void;
  onTimeChange: (ms: number) => void;
}

const rateValues = ["0.5", "0.75", "1", "1.25", "1.5", "2"];
const rateLabel = (v: string) => `${v}x`;

export function VideoControls(props: VideoControlsProps) {
  const currentTime = useCurrentTime();
  const duration = useDuration();
  const fps = useFps();
  const playing = usePlaying();
  const playbackRate = usePlaybackRate();

  const fastForward = () => {
    const newTime = Math.min(currentTime() + 15000, duration());
    props.onTimeChange(newTime);
  };

  const fastRewind = () => {
    const newTime = Math.max(currentTime() - 15000, 0);
    props.onTimeChange(newTime);
  };

  return (
    <div class="flex items-center h-8 px-3 gap-3 border-t text-sm select-none">
      <span class="text-xs text-muted-foreground tabular-nums flex items-center gap-0.5">
        <EditableTimecode
          format="timecode"
          time={currentTime()}
          duration={duration()}
          fps={fps()}
          onTimeChange={props.onTimeChange}
        />
        <span class="text-muted-foreground">(</span>
        <EditableTimecode
          format="ms"
          time={currentTime()}
          duration={duration()}
          fps={fps()}
          onTimeChange={props.onTimeChange}
        />
        <span class="text-muted-foreground">,</span>
        <EditableTimecode
          format="full"
          time={currentTime()}
          duration={duration()}
          fps={fps()}
          onTimeChange={props.onTimeChange}
        />
        <span class="text-muted-foreground">)</span>
        <span class="text-muted-foreground">/</span>
        {msToTimecodeFull(duration(), fps())}
      </span>

      <div class="flex-1 flex justify-center">
        <button
          onClick={props.onTogglePlay}
          class="flex items-center justify-center w-8 h-8 rounded hover:bg-accent/50"
        >
          {playing() ? <Pause size={16} /> : <Play size={16} />}
        </button>
      </div>

      {/* 快进/快退按钮 - 放在倍速选择器旁边 */}
      <div class="flex items-center gap-1">
        <button
          onClick={fastRewind}
          class="flex items-center justify-center w-8 h-8 rounded hover:bg-accent/50"
          title="快退15秒"
        >
          <Rewind size={16} />
        </button>
        <button
          onClick={fastForward}
          class="flex items-center justify-center w-8 h-8 rounded hover:bg-accent/50"
          title="快进15秒"
        >
          <FastForward size={16} />
        </button>
      </div>

      <Select<string>
        options={rateValues}
        value={String(playbackRate())}
        onChange={(v) => props.onRateChange(Number(v))}
        placeholder="1"
        itemComponent={(p) => <SelectItem item={p.item}>{rateLabel(p.item.rawValue)}</SelectItem>}
      >
        <SelectTrigger class="w-14 h-7 text-xs">
          <SelectValue<string>>{(state) => rateLabel(state.selectedOption())}</SelectValue>
        </SelectTrigger>
        <SelectContent />
      </Select>
    </div>
  );
}
