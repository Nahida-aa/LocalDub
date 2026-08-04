import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import { AudioPlayer } from "#/components/ui/audio-player";
import { mediaUrl } from "#/lib/utils/path.ts";
import type { Track, TrackSegment } from "../consts";
import type { TtsSegment } from "@repo/core/stages/07_tts/types";
import type { BaseTrackProps } from "./shared";

type Props = BaseTrackProps;

export function TtsTrack(props: Props) {
  const { track, pxPerMs, onSeek, color, taskDir } = props;

  const handlePlay = (segIndex: number) => {
    const seg = track.segments[segIndex];
    if (!seg) return;
    const raw = seg.raw as TtsSegment | undefined;
    if (!raw || raw.status === "error" || raw.status === "empty") return;
    const idx = String(segIndex + 1).padStart(4, "0");
    const url = mediaUrl(`${taskDir}/tts/wavs/${idx}.wav`);
    const label = `#${segIndex + 1} ${seg.text}`;

    openModal(
      () => (
        <div class="p-4 flex justify-center">
          <AudioPlayer src={url} label={label} />
        </div>
      ),
      { title: `播放 TTS #${segIndex + 1}`, size: "sm" },
    );
  };

  const statusColor = (status: string) => {
    switch (status) {
      case "success":
        return `${color}33`;
      case "error":
        return "#ef444433";
      case "empty":
        return "#6b728033";
      default:
        return `${color}22`;
    }
  };

  const borderColor = (status: string) => {
    switch (status) {
      case "success":
        return `${color}55`;
      case "error":
        return "#ef444455";
      case "empty":
        return "#6b728055";
      default:
        return `${color}33`;
    }
  };

  return (
    <div class="h-16 border-b relative">
      {track.segments.map((seg) => {
        const raw = seg.raw as TtsSegment | undefined;
        const status = raw?.status ?? "skipped";
        return (
          <ContextMenu>
            <ContextMenuTrigger as="div" class="contents">
              <div
                class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
                style={{
                  left: `${seg.startMs * pxPerMs}px`,
                  width: `${Math.max((seg.endMs - seg.startMs) * pxPerMs, 4)}px`,
                  background: statusColor(status),
                  "border-color": borderColor(status),
                }}
                onClick={() => onSeek(seg.startMs)}
                title={`${seg.text} (${status})`}
              >
                {seg.text}
              </div>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem
                onSelect={() => handlePlay(seg.index)}
                disabled={status === "error" || status === "empty"}
              >
                播放 TTS 音频
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        );
      })}
    </div>
  );
}
