import { createSignal, Show, createEffect } from "solid-js";
import { client } from "#/integrations/fnrpc/client.ts";
import { Timeline } from "./Timeline/Timeline";
import type { Track } from "./Timeline/consts";
import { TaskControlPanel } from "#/components/pages/task/TaskControlPanel/TaskControlPanel.tsx";
import { AiReviewPanel } from "#/components/pages/task/AiReviewPanel.tsx";
import { ContentPanel } from "#/components/app/FileContent/ContentPanel";
import { useQuery, useQueryClient } from "@tanstack/solid-query";
import {
  setCurrentTime,
  setDuration,
  setFps,
  setPlaying,
  setPlaybackRate,
  useCurrentTime,
  useDuration,
  useFps,
} from "#/components/app/FileContent/store/videoViewer";
import { useViewingTab } from "./TaskControlPanel/taskControlPanelStore";
import { STAGE_TRACKS, TRACK_DEFS, type TrackDef } from "./Timeline/tracks/const";
import { useTaskTreeEvents } from "./useTaskTreeEvents";
import { bumpMediaVersion } from "#/components/app/FileContent/store/ContentPanel";

interface Props {
  groupId: string;
  taskId: string;
}

export function TaskDetailPage(props: Props) {
  // console.log('[TaskDetailPage] props:', props);
  const taskDir = `workfolder/${props.groupId}/${props.taskId}`;
  const taskCtxQ = useQuery(() => client.get_task_ctx.queryOptions(taskDir));
  // console.log('[TaskDetailPage] taskCtxQ:', taskCtxQ);

  const [videoRef, setVideoRef] = createSignal<HTMLVideoElement | null>(null);
  const qc = useQueryClient();

  // 订阅整棵任务目录文件树：文件变化时刷新对应查询。JSON/文本走 invalidateQueries
  // （TanStack Query 缓存），媒体文件走媒体版本号（axum ServeDir，不进 Query）。
  // 轨道数据由各轨道组件自取（read_app_file_text），这里精确失效对应路径即可，
  // 组件会在查询重建后自动刷新/显隐。
  useTaskTreeEvents(`workfolder/${props.groupId}/${props.taskId}`, {
    onFile: (rel) => {
      // 精确匹配：仅失效与该路径对应的 read_app_file_text 查询，避免惊扰其他文件。
      qc.invalidateQueries({
        queryKey: client.read_app_file_text.queryKey(rel),
      });
      // ctx.json 变化（续跑/运行中阶段状态流转）→ 刷新任务上下文，让 stage 徽章实时更新。
      if (rel.endsWith("ctx.json")) {
        qc.invalidateQueries({
          queryKey: client.get_task_ctx.queryKey(`workfolder/${props.groupId}/${props.taskId}`),
        });
      }
    },
    onMedia: (rel) => {
      bumpMediaVersion(rel);
    },
    onAny: () => {
      // FileTree 目录列表保持最新（root 目录；各 tab 目录由 FileTree 懒挂载时重新拉取）
      qc.invalidateQueries({
        queryKey: client.list_app_directory.queryKey(`workfolder/${props.groupId}/${props.taskId}`),
      });
    },
  });

  createEffect(() => {
    const st = taskCtxQ.status;
    const stages = (taskCtxQ.data?.stages ?? []).map((s) => `${s.name}:${s.status}`).join(",");
    console.warn(`[TRACE-ctx] status=${st} stages=${stages}`);
  });

  const onVideoReady = (ref: HTMLVideoElement) => {
    setVideoRef(ref);
    setDuration(ref.duration * 1000);
    if (taskCtxQ.data) setFps(taskCtxQ.data.frame_rate);
    ref.addEventListener("timeupdate", () => setCurrentTime(ref.currentTime * 1000));
    ref.addEventListener("play", () => setPlaying(true));
    ref.addEventListener("pause", () => setPlaying(false));
  };

  const togglePlay = () => {
    const v = videoRef();
    if (!v) return;
    v.paused ? v.play() : v.pause();
  };
  const onRateChange = (rate: number) => {
    const v = videoRef();
    if (v) v.playbackRate = rate;
    setPlaybackRate(rate);
  };
  const onSeek = (ms: number) => {
    const v = videoRef();
    if (v) v.currentTime = ms / 1000;
  };

  const viewingTab = useViewingTab();

  // 轨道描述符列表是静态的（不含数据）：root 显示全部轨道，其他 tab 仅显示该阶段对应轨道。
  // 各轨道的数据/存在性由轨道组件内部自取，存在（有 segments）才渲染行并上报 label。
  const tracks = (): Track[] => {
    const v = viewingTab();
    const defs: TrackDef[] =
      v === "root"
        ? TRACK_DEFS
        : (STAGE_TRACKS[v] ?? [])
            .map((id) => TRACK_DEFS.find((d) => d.id === id))
            .filter((d): d is TrackDef => !!d);
    return defs.map((d) => ({ id: d.id, label: d.label, segments: [], color: d.color }));
  };

  const duration = useDuration();
  const currentTime = useCurrentTime();
  const fps = useFps();

  return (
    <div class="flex flex-col h-full w-full min-w-0 max-w-full">
      <div class="flex h-120">
        <Show when={taskCtxQ.isPending}>
          <p>Loading...</p>
        </Show>
        <Show when={taskCtxQ.isSuccess}>
          <TaskControlPanel ctx={taskCtxQ.data!} />
        </Show>
        <div class="flex-1 min-w-0 flex flex-col">
          <ContentPanel
            onReady={onVideoReady}
            onTogglePlay={togglePlay}
            onRateChange={onRateChange}
            onTimeChange={onSeek}
          />
        </div>
        <AiReviewPanel />
      </div>
      {/*<Show when={resumeFrom() === 'asr_ocr_pre'}>*/}
      <div class="flex-1 min-h-0">
        <Timeline
          tracks={tracks()}
          duration={duration()}
          currentTime={currentTime()}
          fps={fps()}
          onSeek={onSeek}
          taskDir={taskDir}
        />
      </div>
      {/*</Show>*/}
    </div>
  );
}
