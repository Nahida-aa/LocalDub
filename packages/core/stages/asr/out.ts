import { SubtitleSegment } from "@repo/subtitle/types";
import { AsrArgs } from "./args";

type AsrWord = {
  word: string;
  start: number;
  end: number;
  probability: number;
};

export type AsrSegment = SubtitleSegment & {
  words?: AsrWord[];
  confidence?: {
    avg: number; // 该段平均置信度，范围 [0, 1]
    min: number; // 该段最小置信度，范围 [0, 1]
  };
};

export type AsrResult = {
  result: {
    text: string; // 完整转录文本
    segments: AsrSegment[];
  };
  meta: AsrResultMeta;
};

type AsrResultMeta = {
  audio_duration: number; // 视频总时长，单位 ms
  device: string; // 运行设备，
  detected_language?: string; // 可选的检测到的语言代码，如 "en"、"zh" 等
  engine: string; // whisper.cpp
  model: string; // "/home/aa/repos/env_ls/LocalDub/data/models/whisper/ggml-large-v3-turbo.bin"
  args: AsrArgs;
  input_audio: string; // "/home/aa/repos/env_ls/LocalDub/workfolder/深宫团宠，猫狗皇子皆是我的心头崽（30集）/第3集/separate_after/target_3_vocals_mixed.wav";
  rtf: number; // 0.370
};
