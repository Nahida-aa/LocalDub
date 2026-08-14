import { VoxCPMCloud, VoxCPMNodeONNX } from "@repo/voxlab";
import type { TTSBackend } from "@repo/voxlab";
import { AsyncRetryer } from "@tanstack/pacer";
import { join } from "node:path";
import { to } from "@repo/shared/lib/utils/try.ts";

import { TtsArgs } from "../../stages/07_tts/args";
import { newVoxCPMPyTorchGradio } from "@repo/core/ml/voxcpm/runtime/voxcpm_torch_gradio";

export type { TTSBackend } from "@repo/voxlab";

function createBackend(cfg: TtsArgs): TTSBackend {
  if (!cfg) throw new Error("TTS input not found");
  if (cfg.runtime === "cloud") return new VoxCPMCloud();
  if (cfg.runtime === "voxcpm_torch_gradio") return newVoxCPMPyTorchGradio(cfg);
  const device = cfg.device === "webgpu" ? "webgpu" : "cpu";
  return new VoxCPMNodeONNX({ executionProvider: device });
}

export const newVoxCPMEngine = (cfg: TtsArgs) => {
  let backend: TTSBackend | null = null;
  return {
    async load(): Promise<void> {
      backend = createBackend(cfg);
      await backend.load();
    },
    async synthesize(text: string, refWav: string, promptText?: string): Promise<Float32Array> {
      if (!backend) throw new Error("VoxCPMEngine not loaded");
      const b = backend;
      const retryer = new AsyncRetryer(
        async () => {
          const { samples } = await b.generate({
            text,
            referenceWavPath: refWav,
            promptText,
          });
          return samples;
        },
        {
          onRetry: (attempt, error) => {
            console.log(`[VoxCPM] Retry ${attempt}/3 after:`, error?.message ?? error);
          },
          maxAttempts: 3,
          backoff: "exponential",
          baseWait: 1000, // 失败后距离下次重试的延迟, 如果认为是网络抖动问题，设为 0 也没有关系
          maxWait: 2000,
          jitter: 0.3,
        },
      );
      return (await retryer.execute())!;
    },
    async release(): Promise<void> {
      if (backend) {
        await backend.dispose();
        backend = null;
      }
    },
  };
};
