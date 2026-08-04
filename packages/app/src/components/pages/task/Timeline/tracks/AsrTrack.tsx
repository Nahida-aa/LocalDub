import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import type { Track, TrackSegment } from "../consts";
import { client } from "#/integrations/fnrpc/client.ts";
import { useMutation } from "@tanstack/solid-query";
import { deleteAt, insertAt, type BaseTrackProps } from "./shared";
import { TrackEditModal } from "./comp/TrackEditModal";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[]): string {
  const segs = segments.map((s) => ({
    text: s.text,
    start: s.startMs,
    end: s.endMs,
  }));
  return JSON.stringify({ result: { segments: segs } }, null, 2);
}

export function AsrTrack(props: Props) {
  const { track, pxPerMs, onSeek, color } = props;
  const mutation = useMutation(() =>
    client.write_app_file_text.mutationOptions({
      onSuccess: (_data, variables, _onMutateResult, context) => {
        context.client.invalidateQueries({
          queryKey: client.read_app_file_text.queryKey(variables[0]),
        });
      },
    }),
  );

  const handleInsertBefore = (segIndex: number) => {
    const newSegments = insertAt(track.segments, segIndex, false);
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
  };

  const handleInsertAfter = (segIndex: number) => {
    const newSegments = insertAt(track.segments, segIndex, true);
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = track.segments[segIndex];
    if (!seg) return;

    openModal(
      () => (
        <TrackEditModal
          initialText={seg.text}
          initialStartMs={seg.startMs}
          initialEndMs={seg.endMs}
          onSave={({ text, startMs, endMs }) => {
            const newSegments = track.segments.map((s, i) =>
              i === segIndex ? { ...s, text, startMs, endMs } : s,
            );
            mutation.mutate([props.filePath, serializeSegments(newSegments)]);
          }}
        />
      ),
      { title: "编辑 ASR 片段" },
    );
  };

  const handleDelete = (segIndex: number) => {
    const newSegments = deleteAt(track.segments, segIndex);
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
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
              title={seg.text}
            >
              {seg.text}
            </div>
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem onSelect={() => handleInsertBefore(seg.index)}>
              向前插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleInsertAfter(seg.index)}>
              向后插入
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => handleEdit(seg.index)}>编辑</ContextMenuItem>
            <ContextMenuItem onSelect={() => handleDelete(seg.index)}>删除</ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      ))}
    </div>
  );
}
