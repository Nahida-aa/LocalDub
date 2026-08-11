import { StageName } from "@repo/core/cmd/tasks/input";

// stage 名 → 该阶段对应的轨道 id 列表（一个 stage 可能有多条轨道，且 track.id 不一定等于 stage 名）
export const STAGE_TRACKS: Record<StageName, string[]> = {
  // root: [],
  asr: ["asr"],
  asr_fix: [],
  separate: [],
  separate_after: [],
  asr_ocr_pre: ["asr"],
  asr_ocr: [],
  asr_ocr_fix: ["asr_ocr_fix"],
  sf_ocr_pre: [],
  sf_ocr: [],
  sf_ocr_fix: ["sf_ocr_fix"],
  translate: ["translation", "asr_ocr_fix"],
  split_audio: ["split_audio", "split_audio_timings"],
  tts: ["tts"],
  merge_audio: ["merge_audio"],
  merge_video: [],
};

/// 轨道描述符（静态）：TaskDetailPage 只负责按 tab 过滤后交给 Timeline，
/// 各轨道的数据由轨道组件内部自取；label 在数据存在后可被组件覆盖为精确值。
export interface TrackDef {
  id: string;
  label: string;
  color: string;
}

export const TRACK_DEFS: TrackDef[] = [
  { id: "merge_audio", label: "merge_audio/timings.json", color: "#3b82f6" },
  { id: "tts", label: "tts/tts.json", color: "#f43f5e" },
  { id: "split_audio_timings", label: "split_audio/timings.json", color: "#3b82f6" },
  { id: "split_audio", label: "split_audio/split_audio.json", color: "#f59e0b" },
  { id: "translation", label: "translation", color: "#22c55e" },
  { id: "asr_ocr_fix", label: "asr_ocr_fix/asr_ocr_fused_llm_fix.json", color: "#a855f7" },
  { id: "sf_ocr_fix", label: "sf_ocr_fix", color: "#8b5cf6" },
  { id: "asr", label: "asr.json", color: "#3b82f6" },
];
