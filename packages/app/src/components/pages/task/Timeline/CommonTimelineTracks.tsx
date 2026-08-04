import { For } from "solid-js";
import { CommonTrack, type CommonTrackItem } from "./tracks/CommonTrack";

// ============================================================
//  类型
// ============================================================

/**
 * 一个 track 组：侧边栏一个标签，内容区 N 行。
 *
 * 把原来的 `Track + linkedTrack + linkedTrackB` 三元组，或者任意数量的
 * 关联行，统一收敛成一个 `CommonTrackGroup`。
 */
export interface CommonTrackGroup {
  /** 唯一 ID */
  id: string;
  /** 侧边栏主标签（组名，比如 "字幕校对"） */
  label: string;
  /** 组内每一行，由 CommonTrack 统一渲染 */
  rows: CommonTrackItem[];
  /** 是否联动——多行同步增删改时间轴位置 */
  linked?: boolean;
}

interface Props {
  ref: (el: HTMLDivElement) => void;
  groups: CommonTrackGroup[];
  totalPx: number;
  pxPerMs: number;
  onSeek: (ms: number) => void;
  onScroll: () => void;
  taskDir?: string;
}

// ============================================================
//  辅助：为侧边栏提取标签
// ============================================================

export interface CommonSidebarLabel {
  /** 所属 group 的 id */
  groupId: string;
  /** 这一行在侧边栏显示的文字 */
  label: string;
  /** 左侧边框颜色（来自 row.track.color） */
  color?: string;
}

/**
 * 将 groups 展开为扁平侧边栏标签列表，每行一个标签。
 *
 * 用法示例（在 Timeline 或父组件里）：
 *   const sidebarLabels = () => getCommonTimelineSidebarLabels(groups());
 *   然后在侧边栏里 For each sidebarLabels 画 h-16 的行。
 */
export function getCommonTimelineSidebarLabels(groups: CommonTrackGroup[]): CommonSidebarLabel[] {
  const out: CommonSidebarLabel[] = [];
  for (const g of groups) {
    for (const row of g.rows) {
      out.push({ groupId: g.id, label: row.track.label, color: row.track.color });
    }
  }
  return out;
}

// ============================================================
//  组件
// ============================================================

/**
 * 通用 Timeline 轨道渲染器。
 *
 * 替代原来 `TimelineTracks` 的硬编码分发逻辑（LinkedTTSTrack /
 * SplitAudioLinkedTrack / trackComponents[id]）。
 *
 * 每个 CommonTrackGroup 内部由 CommonTrack 渲染，支持：
 * - 任意行数（不只是固定 1~3 行）
 * - 每行独立的功能开关（features / excluded）
 * - 音频行点击播放
 * - 联动模式（linked=true 时所有行同步增删改）
 * - 自定义序列化格式
 */
export function CommonTimelineTracks(props: Props) {
  return (
    <div ref={props.ref} class="flex-1 overflow-auto min-h-0" onScroll={props.onScroll}>
      <div class="relative" style={{ width: `${props.totalPx}px`, "min-width": "100%" }}>
        <For each={props.groups}>
          {(group) => (
            <CommonTrack
              tracks={group.rows}
              linked={group.linked}
              totalPx={props.totalPx}
              pxPerMs={props.pxPerMs}
              onSeek={props.onSeek}
              taskDir={props.taskDir}
            />
          )}
        </For>
      </div>
    </div>
  );
}
