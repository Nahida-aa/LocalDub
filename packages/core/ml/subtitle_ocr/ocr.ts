import { existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { ocrFrameOpenCvCpp, ocrFramesOpenCvCpp } from "./runtimes/ort-cpp";
import { ocrFramePy } from "./runtimes/ort-py";
import {
  ocrFrameWithSessions,
  createSessions,
  releaseSessions,
  type OCRSessions,
} from "@repo/subtitle-ocr/subtitle-node";
import { REPO_ROOT } from "@repo/config/root";
import { OCRLine } from "@repo/subtitle-ocr/types";
export type { OCRSessions } from "@repo/subtitle-ocr/subtitle-node";
export type OCRDevice = "cpu" | "cuda" | "directml" | "coreml" | "rocm" | "mps";

export type OCRRuntime = "ort-cpp" | "ort-node" | "ort-py";

const BUILD_DIR = resolve(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp", "build");

export function ocrBinaryPath(): string {
  const name = "subtitle_ocr_ort_cpp" + (process.platform === "win32" ? ".exe" : "");
  const candidates = [resolve(BUILD_DIR, "Release", name), resolve(BUILD_DIR, name)];
  return candidates.find((c) => existsSync(c)) || candidates[0];
}

export function ocrOrtDir(): string {
  if (process.platform === "win32") {
    return resolve(
      REPO_ROOT,
      "packages",
      "tmp",
      "onnxruntime-win-x64-1.26.0",
      "onnxruntime-win-x64-1.26.0",
    );
  }
  return "/tmp/onnxruntime-linux-x64-1.24.4";
}

/**
 * OCR engine that manages sessions for ort-node and dispatches per-frame calls.
 */
export const newOcrEngine = async (runtime: OCRRuntime = "ort-cpp", device: OCRDevice = "cpu") => {
  let nodeSessions: OCRSessions | undefined;
  if (runtime === "ort-node") {
    nodeSessions = await createSessions(device);
  }
  return {
    async ocrFrame(
      framePath: string,
      opts?: { textScore?: number; subtitleOnly?: boolean },
    ): Promise<OCRLine[]> {
      switch (runtime) {
        case "ort-cpp":
          return ocrFrameOpenCvCpp(framePath, { ...opts, device });
        case "ort-node":
          if (!nodeSessions) throw new Error("Node sessions not initialized");
          const nodeResult = await ocrFrameWithSessions(framePath, nodeSessions, opts);
          return nodeResult.segments;
        case "ort-py":
          return ocrFramePy(framePath, { ...opts, device });
        default:
          throw new Error(`Unknown OCR runtime: ${runtime}`);
      }
    },
    async ocrFrames(
      frameDir: string,
      frameFiles: string[],
      opts?: { textScore?: number; subtitleOnly?: boolean },
    ): Promise<OCRLine[][]> {
      if (runtime === "ort-cpp") {
        const resultMap = await ocrFramesOpenCvCpp(frameDir, { ...opts, device });
        return frameFiles.map((f) => resultMap.get(f) || []);
      }
      const results: OCRLine[][] = [];
      for (let i = 0; i < frameFiles.length; i++) {
        results.push(await this.ocrFrame(join(frameDir, frameFiles[i]), opts));
      }
      return results;
    },

    async release(): Promise<void> {
      if (runtime === "ort-node" && nodeSessions) {
        await releaseSessions(nodeSessions);
        nodeSessions = undefined;
      }
    },
  };
};

/**
 * Convenience function: creates a one-off engine for a single frame.
 * Prefer OCREngine for batch processing (avoids re-initialising node sessions.
 */
export async function ocrFrame(
  framePath: string,
  runtime: OCRRuntime = "ort-cpp",
  opts?: { textScore?: number; subtitleOnly?: boolean; device?: OCRDevice },
): Promise<OCRLine[]> {
  const engine = await newOcrEngine(runtime, opts?.device ?? "cpu");
  try {
    return await engine.ocrFrame(framePath, opts);
  } finally {
    await engine.release();
  }
}
