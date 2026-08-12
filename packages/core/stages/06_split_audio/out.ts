import { SubtitleSegment } from "@repo/subtitle/types";
import { TranslateSegment } from "../05_translate/out";
import { TargetLang } from "../../cmd/tasks/input";

// start_ms: number; // 视频意图起点
// end_ms: number; // 视频意图终点
export interface SplitAudioTiming extends TranslateSegment {
  seg_idx: number;
}

export type SplitAudioSegment = SplitAudioTiming & {
  split_start_ms: number; // padSegments 切分音频的起点
  split_end_ms: number; // padSegments 切分音频的终点
};

export interface SplitAudioFile {
  translation: SplitAudioSegment[];
}
export interface SplitAudioTimingFile {
  translation: SplitAudioTiming[];
}
