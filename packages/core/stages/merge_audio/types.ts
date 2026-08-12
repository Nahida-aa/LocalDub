import { SplitAudioTiming } from "../06_split_audio/out";

export interface Timing extends SplitAudioTiming {
  original_duration_ms: number; //end_time - start_time(原始时间槽长度
  tts_duration_ms: number; // TTS 生成的音频时长
  stretched_duration_ms: number; // 去尾静音 + rubberband 拉伸后时长
  stretch_ratio: number; // 加速(拉伸)比例(>1.0 = 加速)
  drift_ms: number; //
  advance_ms: number; // 从前面间隙借的时间（实际比 start_time 提前
  delay_ms: number; // 从后面间隙借的时间（实际比 end_time 延后
  actual_start: number; // 实际开始时间（考虑了前面间隙的提前）
  actual_end: number; // 实际结束时间（考虑了后面间隙的延后）
}
export interface TimingsFile {
  translation: Timing[];
}
