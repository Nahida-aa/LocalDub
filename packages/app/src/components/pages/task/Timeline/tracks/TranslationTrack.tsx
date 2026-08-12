import { Show } from "solid-js";
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
import { useMutation, useQuery } from "@tanstack/solid-query";
import type { TranslateFile } from "@repo/core/stages/05_translate/out";
import { deleteAt, insertAt, type BaseTrackProps } from "./shared";
import { TrackEditModal } from "./comp/TrackEditModal";
import { useTrackData } from "./useTrackData";

type Props = BaseTrackProps;

function serializeSegments(segments: TrackSegment[]): string {
  const segs: TranslateFile["translation"] = segments.map((s) => {
    const raw = (s.raw as TranslateFile["translation"][number]) || {};
    return {
      text: raw.text ?? "",
      dst: s.text,
      src_lang: raw.src_lang ?? "auto",
      dst_lang: raw.dst_lang ?? "auto",
      start_ms: s.startMs,
      end_ms: s.endMs,
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

  const ctxQ = useQuery(() => client.get_task_ctx.queryOptions(props.taskDir));
  const lang = () => (ctxQ.isSuccess ? ctxQ.data?.target_language : undefined);
  const path = () => (lang() ? `${props.taskDir}/translate/translation.${lang()}.json` : undefined);

  const { segments } = useTrackData({
    taskDir: props.taskDir,
    trackId: track().id,
    path,
    parse: (text) => {
      const data: TranslateFile = JSON.parse(text);
      return (data.translation || []).map((item, i: number) => ({
        index: i,
        text: item.dst,
        startMs: item.start_ms,
        endMs: item.end_ms,
        raw: item,
      }));
    },
    label: () => `translation.${lang()}.json`,
  });
  const filePath = () => path() ?? "";

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
    const newSegments = insertAt(segments(), segIndex, false, {
      defaultRaw: TRANSLATE_DEFAULT_RAW,
    });
    mutation.mutate([filePath(), serializeSegments(newSegments)]);
  };

  const handleInsertAfter = (segIndex: number) => {
    const newSegments = insertAt(segments(), segIndex, true, {
      defaultRaw: TRANSLATE_DEFAULT_RAW,
    });
    mutation.mutate([filePath(), serializeSegments(newSegments)]);
  };

  const handleEdit = (segIndex: number) => {
    const seg = segments()[segIndex];
    if (!seg) return;
    const raw = seg.raw as TranslateFile["translation"][number] | undefined;

    openModal(
      () => (
        <TrackEditModal
          textLabel="译文"
          srcLabel="原文"
          initialSrc={raw?.text}
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
            const newSegments = segments().map((s, i) =>
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
            mutation.mutate([filePath(), serializeSegments(newSegments)]);
          }}
        />
      ),
      { title: "编辑译文片段" },
    );
  };

  const handleDelete = (segIndex: number) => {
    const newSegments = deleteAt(segments(), segIndex);
    mutation.mutate([filePath(), serializeSegments(newSegments)]);
  };

  return (
    <Show when={segments().length > 0}>
      <div class="h-16 border-b relative">
        {segments().map((seg) => (
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
                title={
                  (seg.raw as TranslateFile["translation"][number] | undefined)?.text || seg.text
                }
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
    </Show>
  );
}
