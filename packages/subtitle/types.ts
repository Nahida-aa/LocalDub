// 字幕  subtitling
export interface SubtitleSegment {
  text: string;
  start_ms: number;
  end_ms: number;
  text_confidence?: number;
}

export interface SrtJson {
  result: {
    text: string;
    segments: SubtitleSegment[];
  };
}
