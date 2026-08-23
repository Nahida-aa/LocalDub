import { OCRRuntime, OcrBoxResult, OcrDevice } from "./types";
import { resolve, join } from "node:path";
import { ocrFrameOpenCvCpp, ocrFramesOpenCvCpp } from "./runtimes/ort-cpp";
/**
 * OCR engine that manages sessions for ort-node and dispatches per-frame calls.
 */
export const newOcrEngine = async (runtime: OCRRuntime = "ort-cpp", device: OcrDevice = "cpu") => {
  return {
    async ocrFrame(
      framePath: string,
      opts?: { textScore?: number; subtitleOnly?: boolean },
    ): Promise<OcrBoxResult[]> {
      switch (runtime) {
        case "ort-cpp":
          return ocrFrameOpenCvCpp(framePath, { ...opts, device });
        // case "ort-node":
        //   if (!nodeSessions) throw new Error("Node sessions not initialized");
        //   const nodeResult = await ocrFrameWithSessions(framePath, nodeSessions, opts);
        //   return nodeResult.segments;
        // case "ort-py":
        //   return ocrFramePy(framePath, { ...opts, device });
        default:
          throw new Error(`Unknown OCR runtime: ${runtime}`);
      }
    },
    async ocrFrames(
      frameDir: string,
      frameFiles: string[],
      opts?: { textScore?: number; subtitleOnly?: boolean },
    ): Promise<OcrBoxResult[][]> {
      if (runtime === "ort-cpp") {
        const resultMap = await ocrFramesOpenCvCpp(frameDir, { ...opts, device });
        return frameFiles.map((f) => resultMap.get(f) || []);
      }
      const results: OcrBoxResult[][] = [];
      for (let i = 0; i < frameFiles.length; i++) {
        results.push(await this.ocrFrame(join(frameDir, frameFiles[i]), opts));
      }
      return results;
    },

    async release(): Promise<void> {},
  };
};

/**
 * Convenience function: creates a one-off engine for a single frame.
 * Prefer OCREngine for batch processing (avoids re-initialising node sessions.
 */
export async function ocrFrame(
  framePath: string,
  runtime: OCRRuntime = "ort-cpp",
  opts?: { textScore?: number; subtitleOnly?: boolean; device?: OcrDevice },
): Promise<OcrBoxResult[]> {
  const engine = await newOcrEngine(runtime, opts?.device ?? "cpu");
  try {
    return await engine.ocrFrame(framePath, opts);
  } finally {
    await engine.release();
  }
}
