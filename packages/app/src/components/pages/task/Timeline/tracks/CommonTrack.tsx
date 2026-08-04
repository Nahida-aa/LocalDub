import { createSignal } from "solid-js";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@repo/ui-solid/base/context-menu";
import { openModal, closeModal } from "@repo/ui-solid/custom/modal/renderer";
import type { Track, TrackSegment } from "../consts";
import { client } from "#/integrations/fnrpc/client.ts";
import { useMutation } from "@tanstack/solid-query";

// ============================================================
//  类型
// ============================================================

/** 可用功能 */
export type TrackFeature = "insert" | "edit" | "delete";

/** 默认功能集 */
const ALL_FEATURES: TrackFeature[] = ["insert", "edit", "delete"];

/** 单个轨道的配置 */
export interface CommonTrackItem {
  /** 轨道数据 */
  track: Track;
  /** 持久化文件路径，不提供则只读（不写盘） */
  filePath?: string;
  /** 颜色，不提供则用 track.color 或默认色 */
  color?: string;
  /** 启用功能列表，不写默认全部 ['insert','edit','delete'] */
  features?: TrackFeature[];
  /** 排除功能列表，从 features 中移除指定项 */
  excluded?: TrackFeature[];
  /** 是否为音频轨道（点击播放音频而非 seek） */
  isAudio?: boolean;
  /** 自定义序列化，不提供用默认 { result: { segments: [{text,start,end}] } } */
  serialize?: (segs: TrackSegment[]) => string;
}

/** 组件 props */
export interface CommonTrackProps {
  /** 任意数量轨道 */
  tracks: CommonTrackItem[];
  /** 是否联动 — 编辑/删除/插入时同步所有轨道同一索引处 */
  linked?: boolean;
  totalPx: number;
  pxPerMs: number;
  onSeek: (ms: number) => void;
  taskDir?: string;
}

// ============================================================
//  工具函数
// ============================================================

const DEFAULT_DURATION_MS = 500;

/** 解析实际生效的功能 */
function resolveFeatures(item: CommonTrackItem): TrackFeature[] {
  const base = item.features ?? ALL_FEATURES;
  const excluded = new Set(item.excluded ?? []);
  return base.filter((f) => !excluded.has(f));
}

/** 默认序列化 */
function defaultSerialize(segments: TrackSegment[]): string {
  return JSON.stringify(
    { result: { segments: segments.map((s) => ({ text: s.text, start: s.startMs, end: s.endMs })) } },
    null,
    2,
  );
}

function serializeItem(item: CommonTrackItem, segs: TrackSegment[]): string {
  return (item.serialize ?? defaultSerialize)(segs);
}

function insertAt(segments: TrackSegment[], index: number, after: boolean): TrackSegment[] {
  const copy = [...segments];
  const current = copy[index];
  if (!current) return copy;

  const newSeg: TrackSegment = {
    index: -1,
    text: "(请在此处修改文本)",
    startMs: after ? current.endMs : Math.max(0, current.startMs - DEFAULT_DURATION_MS),
    endMs: after ? current.endMs + DEFAULT_DURATION_MS : current.startMs,
  };

  if (after) {
    const next = copy[index + 1];
    if (next) {
      const gap = next.startMs - current.endMs;
      newSeg.endMs = current.endMs + Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index + 1, 0, newSeg);
  } else {
    const prev = copy[index - 1];
    if (prev) {
      const gap = current.startMs - prev.endMs;
      newSeg.startMs = current.startMs - Math.min(gap / 2, DEFAULT_DURATION_MS);
    }
    copy.splice(index, 0, newSeg);
  }

  return copy.map((s, i) => ({ ...s, index: i }));
}

function deleteAt(segments: TrackSegment[], index: number): TrackSegment[] {
  return segments.filter((_, i) => i !== index).map((s, i) => ({ ...s, index: i }));
}

/** 检查所有轨道 segments 长度是否一致 */
function allInSync(items: CommonTrackItem[]): boolean {
  if (items.length <= 1) return true;
  const n = items[0].track.segments.length;
  return items.every((it) => it.track.segments.length === n);
}

// ============================================================
//  组件
// ============================================================

export function CommonTrack(props: CommonTrackProps) {
  const items = () => props.tracks;

  // ---- 为每条轨道创建独立 mutation ----
  const mutations: Record<number, ReturnType<typeof useMutation<any, any, any, any>>> = {};
  items().forEach((_item, i) => {
    mutations[i] = useMutation(() =>
      client.write_app_file_text.mutationOptions({
        onMutate: (variables: any, context: any) => {
          context.client.setQueryData(client.read_app_file_text.queryKey(variables[0]), variables[1]);
        },
        onSuccess: (_data: any, variables: any, _onMutateResult: any, context: any) => {
          context.client.invalidateQueries({ queryKey: client.read_app_file_text.queryKey(variables[0]) });
        },
        onError: (error: any) => {
          console.error(`[CommonTrack] write failed for item ${i}:`, error);
        },
      }),
    );
  });

  // ---- 单轨写入 ----
  function writeItem(itemIdx: number, segs: TrackSegment[]) {
    const item = items()[itemIdx];
    if (!item.filePath) return;
    mutations[itemIdx].mutate([item.filePath, serializeItem(item, segs)]);
  }

  // ---- 联动：所有轨道执行同一操作 ----
  function linkedForEach(fn: (item: CommonTrackItem, i: number) => void) {
    items().forEach((it, i) => { if (it.filePath) fn(it, i); });
  }

  // ---- 编辑弹窗 ----
  function handleEdit(itemIdx: number, segIdx: number) {
    const item = items()[itemIdx];
    const seg = item.track.segments[segIdx];
    if (!seg) return;

    // 延迟到下一帧：让 context menu 先完全关闭释放焦点，
    // 避免 Kobalte createHideOutside 因 #root 内仍有焦点元素而
    // 被浏览器阻止设置 aria-hidden
    requestAnimationFrame(() => {
      openModal(
        () => {
          const [text, setText] = createSignal(seg.text);
          const [startMs, setStartMs] = createSignal(seg.startMs);
          const [endMs, setEndMs] = createSignal(seg.endMs);

          const onSave = () => {
            const update = item.track.segments.map((s, i) =>
              i === segIdx ? { ...s, text: text(), startMs: startMs(), endMs: endMs() } : s,
            );

            if (props.linked && allInSync(items())) {
              // 联动模式：对所有轨道同一索引统一更新时间，被编辑的轨道也写入文字
              linkedForEach((it, i) => {
                const synced = it.track.segments.map((s, j) =>
                  j === segIdx
                    ? { ...s, startMs: startMs(), endMs: endMs(), ...(i === itemIdx ? { text: text() } : {}) }
                    : s,
                );
                mutations[i].mutate([it.filePath!, serializeItem(it, synced)]);
              });
            } else if (item.filePath) {
              mutations[itemIdx].mutate([item.filePath, serializeItem(item, update)]);
            }
            closeModal();
          };

          return (
            <div class="flex flex-col gap-3 p-2 text-sm">
              <label class="flex flex-col gap-1">
                <span class="font-medium">文本</span>
                <textarea
                  class="w-full min-h-20 rounded border p-2 text-sm"
                  value={text()}
                  onInput={(e) => setText(e.currentTarget.value)}
                />
              </label>
              <div class="flex gap-4">
                <label class="flex flex-col gap-1 flex-1">
                  <span class="font-medium">开始 (ms)</span>
                  <input
                    class="rounded border px-2 py-1 text-sm"
                    type="number"
                    value={startMs()}
                    onInput={(e) => setStartMs(Number(e.currentTarget.value))}
                  />
                </label>
                <label class="flex flex-col gap-1 flex-1">
                  <span class="font-medium">结束 (ms)</span>
                  <input
                    class="rounded border px-2 py-1 text-sm"
                    type="number"
                    value={endMs()}
                    onInput={(e) => setEndMs(Number(e.currentTarget.value))}
                  />
                </label>
              </div>
              <div class="flex justify-end gap-2 mt-1">
                <button class="px-3 py-1.5 rounded border text-sm cursor-pointer" onClick={closeModal}>
                  取消
                </button>
                <button
                  class="px-3 py-1.5 rounded bg-primary text-primary-foreground text-sm cursor-pointer"
                  onClick={onSave}
                >
                  保存
                </button>
              </div>
            </div>
          );
        },
        { title: `编辑片段 ${segIdx + 1}` },
      );
    });
  }

  // ---- 操作 ----
  function handleInsertBefore(itemIdx: number, segIdx: number) {
    if (props.linked && allInSync(items())) {
      linkedForEach((it, i) => {
        mutations[i].mutate([it.filePath!, serializeItem(it, insertAt(it.track.segments, segIdx, false))]);
      });
    } else {
      const item = items()[itemIdx];
      writeItem(itemIdx, insertAt(item.track.segments, segIdx, false));
    }
  }

  function handleInsertAfter(itemIdx: number, segIdx: number) {
    if (props.linked && allInSync(items())) {
      linkedForEach((it, i) => {
        mutations[i].mutate([it.filePath!, serializeItem(it, insertAt(it.track.segments, segIdx, true))]);
      });
    } else {
      const item = items()[itemIdx];
      writeItem(itemIdx, insertAt(item.track.segments, segIdx, true));
    }
  }

  function handleDelete(itemIdx: number, segIdx: number) {
    if (props.linked && allInSync(items())) {
      linkedForEach((it, i) => {
        mutations[i].mutate([it.filePath!, serializeItem(it, deleteAt(it.track.segments, segIdx))]);
      });
    } else {
      const item = items()[itemIdx];
      writeItem(itemIdx, deleteAt(item.track.segments, segIdx));
    }
  }

  // ---- 音频播放 ----
  const [playing, setPlaying] = createSignal<{ itemIdx: number; segIdx: number } | null>(null);
  let audioEl: HTMLAudioElement | undefined;

  function handlePlay(itemIdx: number, segIdx: number) {
    const seg = items()[itemIdx].track.segments[segIdx];
    if (!seg?.raw) return;
    const url = `http://localhost:19110/media/${seg.raw}`;
    const cur = playing();
    if (cur?.itemIdx === itemIdx && cur?.segIdx === segIdx) {
      audioEl?.pause();
      setPlaying(null);
      return;
    }
    audioEl?.pause();
    audioEl = new Audio(url);
    audioEl.onended = () => setPlaying(null);
    audioEl.play();
    setPlaying({ itemIdx, segIdx });
  }

  // ---- 渲染单列 ----
  function renderRow(item: CommonTrackItem, itemIdx: number) {
    const activeFeatures = resolveFeatures(item);
    const color = item.color ?? item.track.color ?? "#3b82f6";
    const hasInsert = activeFeatures.includes("insert");
    const hasEdit = activeFeatures.includes("edit");
    const hasDelete = activeFeatures.includes("delete");
    const noFeatures = activeFeatures.length === 0;

    return (
      <div class="h-16 border-b relative">
        {item.track.segments.map((seg) => {
          const isActive =
            playing()?.itemIdx === itemIdx && playing()?.segIdx === seg.index;

          return noFeatures ? (
            /* 无功能 → 纯展示，无右键菜单 */
            <div
              class="absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80"
              style={{
                left: `${seg.startMs * props.pxPerMs}px`,
                width: `${Math.max((seg.endMs - seg.startMs) * props.pxPerMs, 4)}px`,
                background: `${color}33`,
                "border-color": `${color}55`,
              }}
              onClick={() =>
                item.isAudio ? handlePlay(itemIdx, seg.index) : props.onSeek(seg.startMs)
              }
              title={seg.text}
            >
              {seg.text}
            </div>
          ) : (
            <ContextMenu>
              <ContextMenuTrigger as="div" class="contents">
                <div
                  class={`absolute top-1 h-12 rounded cursor-pointer truncate text-xs px-2 border flex items-center hover:opacity-80 ${
                    isActive ? "bg-pink-500/30 border-pink-500" : ""
                  }`}
                  style={{
                    left: `${seg.startMs * props.pxPerMs}px`,
                    width: `${Math.max((seg.endMs - seg.startMs) * props.pxPerMs, 4)}px`,
                    background: isActive ? undefined : `${color}33`,
                    "border-color": isActive ? undefined : `${color}55`,
                  }}
                  onClick={() =>
                    item.isAudio ? handlePlay(itemIdx, seg.index) : props.onSeek(seg.startMs)
                  }
                  title={seg.text}
                >
                  {seg.text}
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent>
                {item.isAudio ? (
                  <>
                    <ContextMenuItem onSelect={() => handlePlay(itemIdx, seg.index)}>
                      {isActive ? "⏸ 暂停" : "▶ 播放"}
                    </ContextMenuItem>
                    {hasDelete && (
                      <ContextMenuItem onSelect={() => handleDelete(itemIdx, seg.index)}>
                        删除
                      </ContextMenuItem>
                    )}
                  </>
                ) : (
                  <>
                    {hasInsert && (
                      <ContextMenuItem onSelect={() => handleInsertBefore(itemIdx, seg.index)}>
                        向前插入
                      </ContextMenuItem>
                    )}
                    {hasInsert && (
                      <ContextMenuItem onSelect={() => handleInsertAfter(itemIdx, seg.index)}>
                        向后插入
                      </ContextMenuItem>
                    )}
                    {hasEdit && (
                      <ContextMenuItem onSelect={() => handleEdit(itemIdx, seg.index)}>
                        编辑
                      </ContextMenuItem>
                    )}
                    {hasDelete && (
                      <ContextMenuItem onSelect={() => handleDelete(itemIdx, seg.index)}>
                        删除
                      </ContextMenuItem>
                    )}
                  </>
                )}
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
      </div>
    );
  }

  // ---- 主渲染 ----
  return <>{items().map((item, idx) => renderRow(item, idx))}</>;
}
