import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import { AudioPlayer } from "#/components/ui/audio-player";
import { mediaUrl } from "#/lib/utils/path.ts";
import type { Track, TrackSegment } from "../consts";
import { client } from "#/integrations/fnrpc/client.ts";
import type { SplitAudioTiming } from "@repo/core/stages/06_split_audio/types";
import { TrackEditModal } from "./comp/TrackEditModal";
import { deleteAt, insertAt, type BaseTrackProps } from "./shared";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[]): string {
  const segs: SplitAudioTiming[] = segments.map((s) => {
    const raw = (s.raw as SplitAudioTiming) || ({} as SplitAudioTiming);
    return {
      seg_idx: s.index + 1,
      src: raw.src ?? "",
      dst: s.text,
      src_lang: raw.src_lang ?? "auto",
      dst_lang: raw.dst_lang ?? "vi",
      start: s.startMs,
      end: s.endMs,
      speaker: raw.speaker ?? "1",
    };
  });
  return JSON.stringify({ translation: segs }, null, 2);
}

const SPLIT_AUDIO_DEFAULT_RAW = {
  seg_idx: 0,
  src: "",
  dst: "",
  src_lang: "auto",
  dst_lang: "vi",
  start: 0,
  end: 0,
  speaker: "1",
};

export function SplitAudioTrack(props: Props) {
  const { track, pxPerMs, onSeek, color, taskDir } = props;

  const handleInsertBefore = async (segIndex: number) => {
    const newSegments = insertAt(track.segments, segIndex, false, {
      defaultRaw: SPLIT_AUDIO_DEFAULT_RAW,
    });
    await client.write_app_file_text.call([props.filePath, serializeSegments(newSegments)]);
  };

  const handleInsertAfter = async (segIndex: number) => {
    const newSegments = insertAt(track.segments, segIndex, true, {
      defaultRaw: SPLIT_AUDIO_DEFAULT_RAW,
    });
    await client.write_app_file_text.call([props.filePath, serializeSegments(newSegments)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = track.segments[segIndex];
    if (!seg) return;
    const raw = seg.raw as SplitAudioTiming | undefined;

    openModal(
      () => (
        <TrackEditModal
          textLabel="译文"
          initialText={seg.text}
          initialStartMs={seg.startMs}
          initialEndMs={seg.endMs}
          extraFields={() =>
            raw && (
              <div class="flex gap-4 text-xs text-muted-foreground">
                <span>原文: {raw.src}</span>
                <span>
                  语言: {raw.src_lang}→{raw.dst_lang}
                </span>
              </div>
            )
          }
          onSave={async ({ text, startMs, endMs }) => {
            const newSegments = track.segments.map((s, i) =>
              i === segIndex ? { ...s, text, startMs, endMs } : s,
            );
            await client.write_app_file_text.call([props.filePath, serializeSegments(newSegments)]);
          }}
        />
      ),
      { title: "编辑切分片段" },
    );
  };

  const handleDelete = async (segIndex: number) => {
    const newSegments = deleteAt(track.segments, segIndex);
    await client.write_app_file_text.call([props.filePath, serializeSegments(newSegments)]);
  };

  const handlePlay = (segIndex: number) => {
    const seg = track.segments[segIndex];
    if (!seg) return;
    const idx = String(segIndex + 1).padStart(4, "0");
    const url = mediaUrl(`${taskDir}/split_audio/vocals/${idx}.wav`);
    const label = `#${segIndex + 1} ${seg.text}`;

    openModal(
      () => (
        <div class="p-4 flex justify-center">
          <AudioPlayer src={url} label={label} />
        </div>
      ),
      { title: `播放切分片段 ${label}`, size: "sm" },
    );
  };

  return (
    <div class="h-16 border-b relative">
      {track.segments.map((seg) => (
        <ContextMenu>
          <ContextMenuTrigger as="div" class="contents">
            <div
              class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
              style={{
                left: `${seg.startMs * pxPerMs}px`,
                width: `${Math.max((seg.endMs - seg.startMs) * pxPerMs, 4)}px`,
                background: `${color}33`,
                "border-color": `${color}55`,
              }}
              onClick={() => onSeek(seg.startMs)}
              title={(seg.raw as SplitAudioTiming | undefined)?.src || seg.text}
            >
              {(seg.raw as SplitAudioTiming | undefined)?.src || seg.text}
            </div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem onSelect={() => handlePlay(seg.index)}>播放切分片段</ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => handleInsertBefore(seg.index)}>
              向前插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleInsertAfter(seg.index)}>
              向后插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleEdit(seg.index)}>编辑</ContextMenuItem>
            <ContextMenuItem onSelect={() => onSeek(seg.endMs)}>跳转到结尾</ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => handleDelete(seg.index)} class="text-destructive">
              删除
            </ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      ))}
    </div>
  );
}
