import { SplitAudioTiming } from "../06_split_audio/out";

// start_ms: number; // split_audio start_ms（视频意图位置）
// end_ms: number; // start_ms + tts_duration_ms（TTS 实际结束位置）
export interface TtsSegment extends SplitAudioTiming {
  slot_end_ms: number; // split_audio end_ms（原始槽位终点，参考）
  tts_duration_ms: number;
  status: "success" | "skipped" | "error" | "empty";
}

export interface TtsFile {
  segments: TtsSegment[];
}
