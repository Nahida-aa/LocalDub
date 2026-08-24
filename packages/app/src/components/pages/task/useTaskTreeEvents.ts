import { onCleanup } from "solid-js";
import { consumeEventIterator } from "@fnrpc/client";
import { fnrpc } from "#/integrations/fnrpc/client.ts";
import type { PathEvent } from "@repo/sdk/fnrpc/bindings";
import { warn } from "#/lib/debugLog.ts";

/// 媒体文件扩展名：这些走 axum `/media` ServeDir，不进 TanStack Query，
/// 刷新靠前端给 `<video src>` 追加 `?v=` 版本号强制重拉（见 mediaVersions store）。
const MEDIA_EXTS = ["mp4", "m4v", "mov", "webm", "mkv", "m4a", "wav", "mp3", "ogg", "flac", "aac"];

export function isMediaPath(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return MEDIA_EXTS.includes(ext);
}

export interface TaskTreeHandlers {
  /// 命中 JSON / 文本类文件时调用（路径已转为 workfolder 相对形式）。
  onFile?: (relativePath: string, event: PathEvent) => void;
  /// 命中媒体文件时调用。
  onMedia?: (relativePath: string, event: PathEvent) => void;
  /// 任意事件（已转相对路径）都会调用，用于调试或全量失效。
  onAny?: (relativePath: string, event: PathEvent) => void;
}

/// 把事件路径规整为相对 workfolder 的路径。后端 build_tree_stream 已经把事件
/// path 相对化到 base_dir()（即 `workfolder/group/task/...`），与前端查询用的
/// 相对路径同构。这里做一层兜底：若已以 `workfolder` 开头则直接采用；否则尝试
/// 截取首个 `workfolder` 段；再不行则原样返回。前端不感知 OS 的 base_dir()，所以
/// 绝不应出现绝对路径到达这里。
function toRelativePath(absOrRel: string): string {
  if (absOrRel.startsWith("workfolder")) return absOrRel;
  const idx = absOrRel.indexOf("workfolder");
  if (idx >= 0) return absOrRel.slice(idx);
  return absOrRel;
}

/// 订阅某个 task 目录整棵文件树的变化事件，并分发给 handlers。
///
/// 设计要点（见 .agents/skills/solidjs-reactivity SKILL.md）：
/// - 二进制(媒体)与 JSON 走不同刷新通道，这里只负责事件分发，不读内容。
/// - 前端 debounce 200ms：同一路径在窗口内的多次事件合并为一次回调，
///   与后端 flush debounce 互补，避免流式写入时短暂抖动触发大量 invalidate/刷新。
export function useTaskTreeEvents(taskDir: string, handlers: TaskTreeHandlers) {
  // 把 taskDir 也转成相对形式，用于校验事件是否属于本任务（避免误伤其他任务）。
  const baseRel = toRelativePath(taskDir); // 形如 workfolder/group/task

  const cancel = consumeEventIterator<PathEvent>(fnrpc.watch_task_tree(taskDir), {
    onEvent: (event) => {
      const rel = toRelativePath(event.path);
      // 只处理属于当前任务子树、且确实是文件变化的事件。
      if (!rel.startsWith(baseRel)) return;
      if (event.kind === undefined) return; // Rescan-only / 未知类忽略

      // 同路径 debounce：每个相对路径维护一个计时器，窗口内只触发一次。
      const key = rel;
      const existing = pendingTimers.get(key);
      if (existing) clearTimeout(existing);

      pendingTimers.set(
        key,
        setTimeout(() => {
          pendingTimers.delete(key);
          dispatch(key, event);
        }, DEBOUNCE_MS),
      );

      function dispatch(relativePath: string, ev: PathEvent) {
        handlers.onAny?.(relativePath, ev);
        if (isMediaPath(relativePath)) {
          handlers.onMedia?.(relativePath, ev);
        } else {
          handlers.onFile?.(relativePath, ev);
        }
      }
    },
    onError: (err) => {
      warn("[useTaskTreeEvents] watch error:", err);
    },
  });

  onCleanup(() => cancel());
}

const DEBOUNCE_MS = 200;
const pendingTimers = new Map<string, ReturnType<typeof setTimeout>>();
