import { SubtitlingSegment } from "@repo/subtitling/types";

export interface OcrBoxResult {
  text: string;
  text_confidence: number;
  box: number[][];
  x_range: [number, number];
  y_range: [number, number];
  center: [number, number];
}
export interface FrameResult {
  text: string;
  timestamp: number;
  text_confidence: number;
  x_range: [number, number];
  y_range: [number, number];
  boxes: OcrBoxResult[];
}

export type OCRRuntime = "ort-rust" | "ort-cpp" | "ort-node" | "ort-py";

export type OcrDevice = "cpu" | "cuda" | "directml" | "coreml" | "rocm" | "mps";

// ocr_frames.json 的元数据（溯源/生成参数）
export interface OcrFramesMeta {
  engine: string; // OCRRuntime 名称，如 "ort-cpp"
  device: OcrDevice;
}

// asr_ocr_frames.json | sf_ocr_frames.json | fixed_fps_ocr_frames.json — raw OCR frame output of the asr_ocr stage
export interface OcrFramesResult {
  frames: FrameResult[];
  meta: OcrFramesMeta;
}

export type SegmentFrame = {
  text: string;
  timestamp: number;
  text_confidence: number;
};
export interface OcrSegment extends SubtitlingSegment {
  y_range?: [number, number];
  text_confidence: number;
  frame_count?: number;
  frames?: SegmentFrame[];
}

export interface OcrSegmentWithAdjust extends OcrSegment {
  adjusted_text_confidence?: number;
  y_penalty?: number;
  iso_penalty?: number;
}

export interface AsrOcrBaseSegment {
  text: string;
  start: number;
  end: number;
  box_y: [number, number];
  confidence: number;
}

export interface AsrOcrFile {
  result: {
    segments: AsrOcrBaseSegment[];
  };
}
