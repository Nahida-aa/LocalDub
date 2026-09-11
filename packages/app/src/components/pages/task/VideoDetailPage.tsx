import { createSignal, Show, createEffect } from "solid-js";
import { client } from "#/integrations/fnrpc/client.ts";
import { Timeline } from "./Timeline/Timeline";
import type { Track } from "./Timeline/consts";
import { WorkflowControlPanel } from "#/components/pages/task/WorkflowControlPanel/WorkflowControlPanel.tsx";
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
import { useViewingTab } from "./WorkflowControlPanel/workflowControlPanelStore";
import { STEP_TRACKS, TRACK_DEFS, type TrackDef } from "./Timeline/tracks/const";
import { useWorkflowTreeEvents } from "./useWorkflowTreeEvents";
import {
  addTab,
  bumpMediaVersion,
  setActivePath,
  useActivePath,
} from "#/components/app/FileContent/store/ContentPanel";
import { trace } from "#/lib/debugLog.ts";

/// 绝对路径(OS) → 相对 workfolder 的路径 (镜像 useWorkflowTreeEvents.toRelativePath)。
function toRelPath(absOrRel: string): string {
  if (absOrRel.startsWith("workfolder")) return absOrRel;
  const idx = absOrRel.indexOf("workfolder");
  return idx >= 0 ? absOrRel.slice(idx) : absOrRel;
}

interface Props {
  groupId: string;
  videoId: string;
}

export function VideoDetailPage(props: Props) {
  // console.log('[VideoDetailPage] props:', props);
  const videoDir = `workfolder/${props.groupId}/${props.videoId}`;
  const workflowCtxQ = useQuery(() => client.get_workflow_ctx.queryOptions(videoDir));
  // console.log('[VideoDetailPage] workflowCtxQ:', workflowCtxQ);

  const [videoRef, setVideoRef] = createSignal<HTMLVideoElement | null>(null);
  const qc = useQueryClient();

  // 订阅整棵任务目录文件树：文件变化时刷新对应查询。JSON/文本走 invalidateQueries
  // （TanStack Query 缓存），媒体文件走媒体版本号（axum ServeDir，不进 Query）。
  // 轨道数据由各轨道组件自取（read_app_file_text），这里精确失效对应路径即可，
  // 组件会在查询重建后自动刷新/显隐。
  useWorkflowTreeEvents(`workfolder/${props.groupId}/${props.videoId}`, {
    onFile: (rel) => {
      // 精确匹配：仅失效与该路径对应的 read_app_file_text 查询，避免惊扰其他文件。
      qc.invalidateQueries({
        queryKey: client.read_app_file_text.queryKey(rel),
      });
      // ctx.json 变化（续跑/运行中阶段状态流转）→ 刷新任务上下文，让 step 徽章实时更新。
      if (rel.endsWith("ctx.json")) {
        qc.invalidateQueries({
          queryKey: client.get_workflow_ctx.queryKey(
            `workfolder/${props.groupId}/${props.videoId}`,
          ),
        });
      }
    },
    onMedia: (rel) => {
      bumpMediaVersion(rel);
    },
    onAny: () => {
      // FileTree 目录列表保持最新（root 目录；各 tab 目录由 FileTree 懒挂载时重新拉取）
      qc.invalidateQueries({
        queryKey: client.list_app_directory.queryKey(
          `workfolder/${props.groupId}/${props.videoId}`,
        ),
      });
    },
  });

  createEffect(() => {
    const st = workflowCtxQ.status;
    const steps = (workflowCtxQ.data?.steps ?? []).map((s) => `${s.name}:${s.status}`).join(",");
    trace(`[TRACE-ctx] status=${st} steps=${steps}`);
  });

  // 默认打开本任务视频: 有最终视频(已跑完 mix_video)则用它, 否则 video_source.mp4。
  // 仅在面板空或停留在其他任务的文件时生效; 用户已在本任务打开过文件则不打扰。
  let defaultOpened = false;
  const activePath = useActivePath();
  createEffect(() => {
    const ctx = workflowCtxQ.data;
    if (!ctx || defaultOpened) return;
    defaultOpened = true;
    if (activePath()?.startsWith(videoDir)) return;

    const finalRel = ctx.workflow.final_video_path
      ? toRelPath(ctx.workflow.final_video_path)
      : null;
    const isFinal = !!finalRel?.startsWith(videoDir);
    const defaultRel = isFinal ? finalRel! : `${videoDir}/video_source.mp4`;
    addTab({ path: defaultRel, label: defaultRel.split("/").pop()! });
    setActivePath(defaultRel);
  });

  const onVideoReady = (ref: HTMLVideoElement) => {
    setVideoRef(ref);
    setDuration(ref.duration * 1000);
    if (workflowCtxQ.data) setFps(workflowCtxQ.data.frame_rate);
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
        : (STEP_TRACKS[v] ?? [])
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
        <Show when={workflowCtxQ.isPending}>
          <p>Loading...</p>
        </Show>
        <Show when={workflowCtxQ.isSuccess}>
          <WorkflowControlPanel ctx={workflowCtxQ.data!} />
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
          videoDir={videoDir}
        />
      </div>
      {/*</Show>*/}
    </div>
  );
}
