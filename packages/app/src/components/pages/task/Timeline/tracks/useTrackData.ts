import { createEffect, createMemo } from "solid-js";
import { useQuery } from "@tanstack/solid-query";
import { client } from "#/integrations/fnrpc/client.ts";
import type { TrackSegment } from "../consts";
import { setTrackMeta } from "./presence";

export interface TrackDataOptions {
  taskDir: string;
  trackId: string;
  /// 文件相对路径 getter；返回 undefined 时查询保持禁用（例如译文路径依赖 ctx 的 target_language）。
  path: () => string | undefined;
  /// 可选回退路径：仅当主路径读取失败时读取（用于 sf_ocr_fix 的 LLM 修正 → 段过滤回退）。
  fallbackPath?: () => string | undefined;
  parse: (text: string) => TrackSegment[];
  /// 数据存在时的 label（缺省沿用轨道描述符 label）。
  label?: () => string;
}

/// 禁用态查询占位：路径未知时保持挂起、不发起请求。
const disabledOptions = (trackId: string) => ({
  queryKey: [trackId],
  queryFn: async () => "",
  enabled: false,
});

/// 轨道自取数据：内部发起 read_app_file_text，segments 一律在 isSuccess 守卫内
/// 读取（pending 期不触碰 q.data，避免向 router 隐式 Suspense 注册导致整页空白）。
/// 读取失败（文件不存在）即视为轨道不存在 → present=false → 轨道隐藏。
export function useTrackData(opts: TrackDataOptions) {
  const q = useQuery(() => {
    const p = opts.path();
    if (!p) return disabledOptions(opts.trackId);
    return client.read_app_file_text.queryOptions(p, { enabled: true, retry: false });
  });
  const fb = useQuery(() => {
    const fp = opts.fallbackPath?.();
    if (!fp) return disabledOptions(`${opts.trackId}:fb`);
    return client.read_app_file_text.queryOptions(fp, { enabled: q.isError, retry: false });
  });

  const active = createMemo(() => {
    if (q.isSuccess) return q;
    if (fb.isSuccess) return fb;
    return null;
  });

  const segments = createMemo<TrackSegment[]>(() => {
    const a = active();
    if (!a) return [];
    const text = a.data;
    if (!text) return [];
    try {
      return opts.parse(text);
    } catch {
      return [];
    }
  });
  const present = createMemo(() => segments().length > 0);

  createEffect(() => {
    setTrackMeta(opts.taskDir, opts.trackId, {
      present: present(),
      label: present() ? opts.label?.() : undefined,
    });
  });

  return { q, fb, active, segments, present };
}
