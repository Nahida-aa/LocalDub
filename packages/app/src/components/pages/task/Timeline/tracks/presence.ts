import { createStore, useSelector } from "@tanstack/solid-store";

export interface TrackMeta {
  present: boolean;
  label?: string;
}

/// 轨道存在性元数据：由各轨道组件自身上报（present 由数据读取结果推导），
/// TimelineTrackSide 据此决定 label 行是否渲染，与轨道内容行保持 1:1 对齐。
/// key 含 workflowDir，避免跨任务残留。
const trackMetaStore = createStore<Record<string, TrackMeta>>({});

export function useTrackMetaRecord() {
  return useSelector(trackMetaStore);
}

export function setTrackMeta(workflowDir: string, trackId: string, meta: TrackMeta) {
  trackMetaStore.setState((s) => ({ ...s, [`${workflowDir}/${trackId}`]: meta }));
}
