import { createSignal, Match, Show, Switch, createEffect } from "solid-js";
import { client } from "#/integrations/fnrpc/client.ts";
import { Timeline } from "./Timeline/Timeline";
import type { Track } from "./Timeline/consts";
import { TaskControlPanel } from "#/components/pages/task/TaskControlPanel/TaskControlPanel.tsx";
import { AiReviewPanel } from "#/components/pages/task/AiReviewPanel.tsx";
import { ContentPanel } from "#/components/app/FileContent/ContentPanel";
import { createQuery, useQuery, useQueryClient } from "@tanstack/solid-query";
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
import { to } from "@repo/shared/lib/utils/try";
import {
  SplitAudioFile,
  SplitAudioTiming,
  SplitAudioTimingFile,
} from "@repo/core/stages/06_split_audio/types";
import type { TtsFile } from "@repo/core/stages/07_tts/types";
import { TimingsFile } from "@repo/core/stages/merge_audio/types";
import {
  use_resumeFrom,
  useViewingTab,
  type StageTab,
} from "./TaskControlPanel/taskControlPanelStore";
import { STAGE_TRACKS } from "./Timeline/tracks/const";
import { useTaskTreeEvents, useFileExists } from "./useTaskTreeEvents";
import { bumpMediaVersion } from "#/components/app/FileContent/store/ContentPanel";
import { AsrOcrFile } from "@repo/subtitle-ocr/types";
import { AsrResult } from "@repo/subtitle-asr/types";
import { OcrSegmentFilterResult } from "@repo/subtitle-ocr/ocr_fix/segment_filter";

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
  // const watch_task_tree_q = useQuery(() =>
  //   client.watch_task_tree.streamedOptions(`workfolder/${props.groupId}/${props.taskId}`),
  // );

  // 订阅整棵任务目录文件树：文件变化时刷新对应查询。JSON/文本走 invalidateQueries
  // （TanStack Query 缓存），媒体文件走媒体版本号（axum ServeDir，不进 Query）。
  // 另：任意事件都刷新目录列表，让 useFileExists 的存在性判断保持最新。
  useTaskTreeEvents(`workfolder/${props.groupId}/${props.taskId}`, {
    onFile: (rel) => {
      // 精确匹配：仅失效与该路径对应的 read_app_file_text 查询，避免惊扰其他文件。
      qc.invalidateQueries({
        queryKey: client.read_app_file_text.queryKey(rel),
      });
    },
    onMedia: (rel) => {
      bumpMediaVersion(rel);
    },
    onAny: () => {
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

  const transLang = () => taskCtxQ.data?.target_language;
  // 文件存在性判断（基于目录列表，而非 stage status）——决定查询是否发起首次请求。
  // 文件**内容**改写后的刷新由上面的 useTaskTreeEvents onFile 回调负责（invalidate），
  // 二者分工不同（详见 .agents/skills/solidjs-reactivity SKILL.md 的 `enabled` 陷阱）。
  const asrExists = useFileExists(taskDir, "asr/asr.json");
  const transExists = useFileExists(taskDir, () => `translate/translation.${transLang()}.json`);
  const mergeAudioExists = useFileExists(taskDir, "merge_audio/timings.json");
  const splitAudioTimingsExists = useFileExists(taskDir, "split_audio/timings.json");
  const splitAudioExists = useFileExists(taskDir, "split_audio/split_audio.json");
  const ttsExists = useFileExists(taskDir, "tts/tts.json");
  const asrOcrFixExists = useFileExists(taskDir, "asr_ocr_fix/asr_ocr_fused_llm_fix.json");
  const sfOcrFixLlmExists = useFileExists(taskDir, "sf_ocr_fix/segment_filter_llm_fix.json");
  const sfOcrFixExists = useFileExists(taskDir, "sf_ocr_fix/segment_filter.json");

  const asrQuery = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/asr/asr.json`, {
      enabled: asrExists(),
    }),
  );

  const asrSegments = () => {
    if (!asrQuery.data) return [];
    try {
      const data: AsrResult = JSON.parse(asrQuery.data);
      return (data.result?.segments || [])
        .map((s, i: number) => ({
          index: i,
          text: (s.text || "").trim(),
          startMs: s.start_ms,
          endMs: s.end_ms,
        }))
        .filter((s: { text: string }) => s.text);
    } catch {
      return [];
    }
  };

  const transQuery = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/translate/translation.${transLang()}.json`, {
      enabled: !!transLang() && transExists(),
    }),
  );
  const transSegments = () => {
    if (!transQuery.data) return [];
    try {
      const data: TranslateFile = JSON.parse(transQuery.data);
      return (data.translation || []).map((item, i: number) => ({
        index: i,
        text: item.dst,
        startMs: item.start_ms,
        endMs: item.end_ms,
      }));
    } catch {
      return [];
    }
  };

  const merge_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/merge_audio/timings.json`, {
      enabled: mergeAudioExists(),
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
      raw: item,
    }));
  };

  const split_audio_timings_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/split_audio/timings.json`, {
      enabled: splitAudioTimingsExists(),
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
      enabled: splitAudioExists(),
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
      enabled: ttsExists(),
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
      enabled: asrOcrFixExists(),
    }),
  );
  const asr_ocr_fix_llm = () => {
    if (!asr_ocr_fix_llm_q.data) return [];
    const [data, err] = to<AsrOcrFile>(() => JSON.parse(asr_ocr_fix_llm_q.data));
    if (err) return [];
    return data.result.segments.map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };

  // sf_ocr_fix 轨道：优先 LLM 修正产物，否则回落到段过滤产物
  const sfOcrFixPath = () =>
    sfOcrFixLlmExists()
      ? "sf_ocr_fix/segment_filter_llm_fix.json"
      : "sf_ocr_fix/segment_filter.json";
  const sfOcrFixQ = useQuery(() =>
    client.read_app_file_text.queryOptions(`${taskDir}/${sfOcrFixPath()}`, {
      enabled: sfOcrFixLlmExists() || sfOcrFixExists(),
    }),
  );
  const sfOcrFixSegments = () => {
    if (!sfOcrFixQ.data) return [];
    const [data, err] = to<OcrSegmentFilterResult>(() => JSON.parse(sfOcrFixQ.data));
    if (err) return [];
    return (data.result?.segments ?? []).map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start_ms,
      endMs: item.end_ms,
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
        filePath: `${taskDir}/merge_audio/timings.json`,
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
    const sfOcrFix = sfOcrFixSegments();
    if (sfOcrFix.length)
      result.push({
        id: "sf_ocr_fix",
        label: sfOcrFixPath(),
        segments: sfOcrFix,
        color: "#8b5cf6",
        filePath: `${taskDir}/${sfOcrFixPath()}`,
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
