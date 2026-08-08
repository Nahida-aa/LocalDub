export interface FrameResult {
  text: string;
  timestamp: number;
  confidence: number;
  x_range?: [number, number];
  y_range?: [number, number];
  boxes: OcrBoxResult[];
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
