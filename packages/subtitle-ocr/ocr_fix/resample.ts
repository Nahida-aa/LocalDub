import { FrameResult } from "../types";

export const hasNearbySameText = (
  rawFrames: FrameResult[],
  i: number,
  f: FrameResult,
  resampleRangeMs = 500,
) => {
  return rawFrames.some(
    (other, j) =>
      j !== i &&
      other.text === f.text &&
      Math.abs(other.timestamp - f.timestamp) <= resampleRangeMs,
  );
};
