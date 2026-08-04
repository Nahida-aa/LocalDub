import { createSignal, Match, Show, Switch, createEffect } from "solid-js";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import { Timeline } from "./Timeline/Timeline";
import type { Track } from "./Timeline/consts";
import { TaskControlPanel } from "#/components/pages/task/TaskControlPanel/TaskControlPanel.tsx";
import { AiReviewPanel } from "#/components/pages/task/AiReviewPanel.tsx";
import { ContentPanel } from "#/components/app/FileContent/ContentPanel";
import { createQuery, useQuery } from "@tanstack/solid-query";
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
import { TranslateFile } from "@repo/core/stages/05_translate/type";
import { AsrResult } from "@repo/core/stages/asr/types";
import { stages_to_map } from "@repo/core/stages/utils/filtering";
import { to } from "@repo/shared/lib/utils/try";
import {
  SplitAudioFile,
  SplitAudioTiming,
  SplitAudioTimingFile,
} from "@repo/core/stages/06_split_audio/types";
import { AsrOcrFile } from "@repo/core/ml/subtitle_ocr/types";
import type { TtsFile } from "@repo/core/stages/07_tts/types";
import { TimingsFile } from "@repo/core/stages/merge_audio/types";
import {
  use_resumeFrom,
  useViewingTab,
  type StageTab,
} from "./TaskControlPanel/taskControlPanelStore";
import { STAGE_TRACKS } from "./Timeline/tracks/const";

interface Props {
  groupId: string;
  taskId: string;
}

export function TaskDetailPage(props: Props) {
  // console.log('[TaskDetailPage] props:', props);
  const taskDir = `workfolder/${props.groupId}/${props.taskId}`;
  const taskCtxQ = useQuery(() => client.get_task_ctx.queryOptions(taskDir));
  // console.log('[TaskDetailPage] taskCtxQ:', taskCtxQ);
  const stage_map = () => stages_to_map(taskCtxQ.data?.stages ?? []);
  // console.log('[TaskDetailPage] stages:', taskCtxQ.data?.stages?.map(s => s.name));

  const [videoRef, setVideoRef] = createSignal<HTMLVideoElement | null>(null);
  const resumeFrom = use_resumeFrom();
  const watch_task_tree_q = useQuery(() =>
    client.watch_task_tree.streamedOptions(`workfolder/${props.groupId}/${props.taskId}`),
  );

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

  const asrQuery = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/asr/asr.json`, {
      enabled: stage_map().asr?.status === "success",
    }),
  );

  const asrSegments = () => {
    if (!asrQuery.data) return [];
    try {
      const data: AsrResult = JSON.parse(asrQuery.data);
      return (data.result?.segments || [])
        .map((s: any, i: number) => ({
          index: i,
          text: (s.text || "").trim(),
          startMs: s.start,
          endMs: s.end,
        }))
        .filter((s: { text: string }) => s.text);
    } catch {
      return [];
    }
  };

  const transLang = () => taskCtxQ.data?.target_language;
  const transQuery = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/translate/translation.${transLang()}.json`, {
      enabled: stage_map().translate?.status === "success",
    }),
  );
  const transSegments = () => {
    if (!transQuery.data) return [];
    try {
      const data: TranslateFile = JSON.parse(transQuery.data);
      return (data.translation || []).map((item, i: number) => ({
        index: i,
        text: item.dst || "",
        startMs: item.start,
        endMs: item.end,
      }));
    } catch {
      return [];
    }
  };

  const merge_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/merge_audio/timings.json`, {
      enabled: stage_map().merge_audio?.status === "success",
    }),
  );
  const merge_audio_segments = () => {
    if (!merge_audio_q.data) return [];
    const [data, err] = to<TimingsFile>(() => JSON.parse(merge_audio_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.actual_start,
      endMs: item.actual_end,
    }));
  };

  const split_audio_timings_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/split_audio/timings.json`, {
      enabled: stage_map().split_audio?.status === "success",
    }),
  );
  const split_audio_timings = () => {
    if (!split_audio_timings_q.data) return [];
    const [data, err] = to<SplitAudioTimingFile>(() => JSON.parse(split_audio_timings_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst || "",
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };

  const split_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/split_audio/split_audio.json`, {
      enabled: stage_map().split_audio?.status === "success",
    }),
  );
  const split_audio = () => {
    if (!split_audio_q.data) return [];
    const [data, err] = to<SplitAudioFile>(() => JSON.parse(split_audio_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst || "",
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };

  const ttsQ = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/tts/tts.json`, {
      enabled: stage_map().tts?.status === "success",
    }),
  );
  const ttsSegments = () => {
    if (!ttsQ.data) return [];
    const [data, err] = to<TtsFile>(() => JSON.parse(ttsQ.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };

  const asr_ocr_fix_llm_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/asr_ocr_fix/asr_ocr_fused_llm_fix.json`, {
      enabled: stage_map().asr_ocr_fix?.status === "success",
    }),
  );
  const asr_ocr_fix_llm = () => {
    if (!asr_ocr_fix_llm_q.data) return [];
    const [data, err] = to<AsrOcrFile>(() => JSON.parse(asr_ocr_fix_llm_q.data));
    if (err) return [];
    return data.result.segments.map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };

  const viewingTab = useViewingTab();

  const tracks = (): Track[] => {
    const result: Track[] = [];
    const merge_audio = merge_audio_segments();
    if (merge_audio.length)
      result.push({
        id: "merge_audio",
        label: "merge_audio/timings.json",
        segments: merge_audio,
        color: "#3b82f6",
      });
    const tts = ttsSegments();
    if (tts.length)
      result.push({
        id: "tts",
        label: "tts/tts.json",
        segments: tts,
        color: "#f43f5e",
        filePath: `${taskDir}/tts/tts.json`,
      });
    const split_audio_timings_data = split_audio_timings();
    if (split_audio_timings_data.length)
      result.push({
        id: "split_audio_timings",
        label: "split_audio/timings.json",
        segments: split_audio_timings_data,
        color: "#3b82f6",
        filePath: `${taskDir}/split_audio/timings.json`,
      });
    const split_audio_data = split_audio();
    if (split_audio_data.length)
      result.push({
        id: "split_audio",
        label: "split_audio/split_audio.json",
        segments: split_audio_data,
        color: "#f59e0b",
        filePath: `${taskDir}/split_audio/split_audio.json`,
      });
    const trans = transSegments();
    if (trans.length)
      result.push({
        id: "translation",
        label: `translation.${transLang()}.json`,
        segments: trans,
        color: "#22c55e",
        filePath: `${taskDir}/translate/translation.${transLang()}.json`,
      });
    const asr_ocr_fix_llm_ = asr_ocr_fix_llm();
    if (asr_ocr_fix_llm_.length)
      result.push({
        id: "asr_ocr_fix",
        label: "asr_ocr_fix/asr_ocr_fused_llm_fix.json",
        segments: asr_ocr_fix_llm_,
        color: "#a855f7",
        filePath: `${taskDir}/asr_ocr_fix/asr_ocr_fused_llm_fix.json`,
      });
    const asr = asrSegments();
    if (asr.length) result.push({ id: "asr", label: "asr.json", segments: asr, color: "#3b82f6" });

    // root 显示全部轨道；其他 tab 仅显示该阶段对应的轨道
    const v = viewingTab();
    if (v === "root") return result;
    const ids = STAGE_TRACKS[v] ?? [];
    return ids.length ? result.filter((t) => ids.includes(t.id)) : [];
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
          <TaskControlPanel
            ctx={taskCtxQ.data!}
            // resumeFromStage={resumeFromStage()}
            // onResumeFrom={setResumeFromStage}
          />
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
      <div class="flex-1">
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
