import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import type { Track, TrackSegment } from "../consts";
import { client } from "#/integrations/fnrpc/client.ts";
import { useMutation } from "@tanstack/solid-query";
import type { TranslateFile } from "@repo/core/stages/05_translate/type";
import { deleteAt, insertAt, type BaseTrackProps } from "./shared";
import { TrackEditModal } from "./comp/TrackEditModal";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[]): string {
  const segs: TranslateFile["translation"] = segments.map((s) => {
    const raw = (s.raw as TranslateFile["translation"][number]) || {};
    return {
      src: raw.src ?? "",
      dst: s.text,
      src_lang: raw.src_lang ?? "auto",
      dst_lang: raw.dst_lang ?? "auto",
      start: s.startMs,
      end: s.endMs,
      speaker: raw.speaker ?? "1",
    };
  });
  return JSON.stringify({ translation: segs }, null, 2);
}

const TRANSLATE_DEFAULT_RAW = {
  src: "",
  dst: "",
  src_lang: "auto",
  dst_lang: "auto",
  start: 0,
  end: 0,
  speaker: "1",
};

export function TranslationTrack(props: Props) {
  const track = () => props.track;
  const pxPerMs = () => props.pxPerMs;
  const color = () => props.color;
  const onSeek = props.onSeek;

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
    const newSegments = insertAt(track().segments, segIndex, false, {
      defaultRaw: TRANSLATE_DEFAULT_RAW,
    });
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
  };

  const handleInsertAfter = (segIndex: number) => {
    const newSegments = insertAt(track().segments, segIndex, true, {
      defaultRaw: TRANSLATE_DEFAULT_RAW,
    });
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = track().segments[segIndex];
    if (!seg) return;
    const raw = seg.raw as TranslateFile["translation"][number] | undefined;

    openModal(
      () => (
        <TrackEditModal
          textLabel="译文"
          srcLabel="原文"
          initialSrc={raw?.src}
          initialText={seg.text}
          initialStartMs={seg.startMs}
          initialEndMs={seg.endMs}
          extraFields={() =>
            raw && (
              <div class="flex gap-4 text-xs text-muted-foreground">
                <span>
                  语言: {raw.src_lang}→{raw.dst_lang}
                </span>
              </div>
            )
          }
          onSave={({ text, src, startMs, endMs }) => {
            const newSegments = track().segments.map((s, i) =>
              i === segIndex
                ? {
                    ...s,
                    text,
                    startMs,
                    endMs,
                    raw: { ...(s.raw as object), ...(src !== undefined ? { src } : {}) },
                  }
                : s,
            );
            mutation.mutate([props.filePath, serializeSegments(newSegments)]);
          }}
        />
      ),
      { title: "编辑译文片段" },
    );
  };

  const handleDelete = (segIndex: number) => {
    const newSegments = deleteAt(track().segments, segIndex);
    mutation.mutate([props.filePath, serializeSegments(newSegments)]);
  };

  return (
    <div class="h-16 border-b relative">
      {track().segments.map((seg) => (
        <ContextMenu>
          <ContextMenuTrigger as="div" class="contents">
            <div
              class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
              style={{
                left: `${seg.startMs * pxPerMs()}px`,
                width: `${Math.max((seg.endMs - seg.startMs) * pxPerMs(), 4)}px`,
                background: `${color()}33`,
                "border-color": `${color()}55`,
              }}
              onClick={() => onSeek(seg.startMs)}
              title={(seg.raw as TranslateFile["translation"][number] | undefined)?.src || seg.text}
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
