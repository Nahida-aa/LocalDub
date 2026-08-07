export interface FrameResult {
  text: string;
  timestamp: number;
  confidence: number;
  x_range?: [number, number];
  y_range?: [number, number];
  boxes?: OcrBoxResult[];
}

export interface OcrBoxResult {
  text: string;
  text_confidence: number;
  box: number[][];
  x_range: [number, number];
  y_range: [number, number];
  center: [number, number];
}

export type OcrDevice = "cpu" | "cuda" | "directml" | "coreml" | "rocm" | "mps";

// 前处理(抽帧)策略：asr=ASR 分段驱动, sf=关键帧直接抠字幕, fixed_fps=固定 fps 抽帧
export type PreprocessMode = "asr" | "sf" | "fixed_fps";

// ocr_frames.json 的元数据（溯源/生成参数）
export interface OcrFramesMeta {
  video_duration_ms: number; // 视频(媒体)总时长，单位 ms
  preprocess: PreprocessMode; // 前处理(抽帧)策略
  engine: string; // OCRRuntime 名称，如 "ort-cpp"
  device: OcrDevice;
}

// ocr_frames.json — raw OCR frame output of the asr_ocr stage
export interface OcrFramesResult {
  frames: FrameResult[];
  meta: OcrFramesMeta;
}
