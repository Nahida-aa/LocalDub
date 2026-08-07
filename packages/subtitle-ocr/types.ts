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
  confidence: number;
  box: number[][];
  x_range: [number, number];
  y_range: [number, number];
  center: [number, number];
}
