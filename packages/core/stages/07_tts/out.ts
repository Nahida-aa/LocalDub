export interface TtsSegment {
  seg_idx: number;
  text: string;
  start: number; // split_audio start_time（视频意图位置）
  end: number; // start + tts_duration_ms（TTS 实际结束位置）
  slot_end: number; // split_audio end_time（原始槽位终点，参考）
  tts_duration_ms: number;
  status: "success" | "skipped" | "error" | "empty";
}

export interface TtsFile {
  segments: TtsSegment[];
}
