import { useParams } from "@tanstack/solid-router";
import { useViewingTab } from "./TaskControlPanel/taskControlPanelStore";
import {
  use_asr_data,
  use_asr_ocr_fix_llm_data,
  use_mix_audio_timings_data,
  use_split_audio_data,
  use_split_audio_timings_data,
  use_task_ctx,
  use_translate_data,
  use_tts_data,
} from "./query";
import { stages_to_map } from "@repo/core/stages/utils/filtering";
import type { Track } from "./Timeline/consts";
import { STAGE_TRACKS } from "./Timeline/tracks/const";

/** 当前任务的 timeline 轨道集合（返回 getter），按 viewingTab 过滤 */
export const use_track = (): (() => Track[]) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const taskDir = `workfolder/${p().id}/${p().taskId}`;
  const taskCtxQ = use_task_ctx();
  const stage_map = () => stages_to_map(taskCtxQ.data?.stages ?? []);
  const viewingTab = useViewingTab();

  const asrSegments = use_asr_data({ enabled: () => stage_map().asr?.status === "success" });
  const split_audio_timings = use_split_audio_timings_data({
    enabled: () => stage_map().split_audio?.status === "success",
  });
  const ttsSegments = use_tts_data({ enabled: () => stage_map().tts?.status === "success" });
  const transLang = () => taskCtxQ.data?.target_language;
  const transSegments = use_translate_data({
    enabled: () => !!transLang() && stage_map().translate?.status === "success",
  });
  const mix_audio_segments = use_mix_audio_timings_data({
    enabled: () => stage_map().mix_audio?.status === "success",
  });
  const split_audio = use_split_audio_data({
    enabled: () => stage_map().split_audio?.status === "success",
  });
  const asr_ocr_fix_llm = use_asr_ocr_fix_llm_data({
    enabled: () => stage_map().asr_ocr_fix?.status === "success",
  });

  return (): Track[] => {
    const result: Track[] = [];
    const mix_audio = mix_audio_segments();
    if (mix_audio.length)
      result.push({
        id: "mix_audio",
        label: "mix_audio/timings.json",
        segments: mix_audio,
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
};
