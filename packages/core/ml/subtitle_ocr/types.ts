export type SegmentFrame = {
  text: string;
  timestamp: number;
  confidence: number;
};
export interface Segment {
  text: string;
  start: number;
  end: number;
  start_fmt?: string;
  end_fmt?: string;
  box_y?: [number, number];
  confidence?: number;
  frameCount?: number;
  frames?: SegmentFrame[];
}

export interface SegmentWithAdjusted extends Segment {
  adjustedConfidence?: number;
  yPenalty?: number;
  isoPenalty?: number;
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
