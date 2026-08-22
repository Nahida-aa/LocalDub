import { StageName } from "@repo/core/cmd/tasks/input";
import { StageTab } from "../../TaskControlPanel/taskControlPanelStore";

// stage 名 → 该阶段对应的轨道 id 列表（一个 stage 可能有多条轨道，且 track.id 不一定等于 stage 名）
export const STAGE_TRACKS: Record<StageName, string[]> = {
  // root: [],
  asr: ["asr"],
  asr_fix: [],
  import_subtitle: [],
  separate: [],
  separate_after: [],
  asr_ocr_pre: ["asr"],
  asr_ocr: [],
  asr_ocr_fix: ["asr_ocr_fix"],
  ocr: [],
  ocr_fix: [],
  translate: ["translation", "asr_ocr_fix"],
  split_audio: ["split_audio", "split_audio_timings"],
  tts: ["tts"],
  merge_audio: ["merge_audio"],
  merge_video: [],
};
