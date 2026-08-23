import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, renameSync } from "node:fs";
import { delimiter, join, resolve, basename } from "node:path";
import { homedir } from "node:os";

import { pythonBin } from "@repo/config/path/bin";
import { REPO_ROOT } from "@repo/config/root";
import { AsrOptions } from "@repo/core/stages/asr/types.ts";
import { setCtx, setStage } from "@repo/core/context/context.ts";
import { parseAsrOutput } from "@repo/core/stages/asr/utils.ts";
import { readJson } from "@repo/core/utils/fileOps";
import { emitAsrTiming } from "../time.ts";
import { faster_whisper_py } from "@repo/config/path/scripts";

export async function asrFasterWhisper(opts: AsrOptions) {
  const { taskId, audioPath, taskDir: taskDir, language, device, pythonBin: pyBin, ctx } = opts;
  const baseArgs = [faster_whisper_py, audioPath, taskDir, language || "auto"];

  const useGpu = device !== "cpu";
  const attempts = useGpu ? 2 : 1;
  let fallbackToCpu = false;

  for (let attempt = 0; attempt < attempts; attempt++) {
    const args = attempt === 0 && useGpu ? baseArgs : [...baseArgs, "--cpu"];
    const t0 = Date.now();
    const result = spawnSync(pyBin, args, {
      maxBuffer: 256 * 1024 * 1024,
      timeout: 600_000,
    });
    const elapsedSec = (Date.now() - t0) / 1000;

    if (result.signal) {
      const stderr = (result.stderr?.toString() || "").trim().slice(-200);
      if (attempt === 0 && useGpu) {
        await setStage(taskDir, "asr", {
          last_message: "GPU hang, retrying CPU...",
        });
        fallbackToCpu = true;
        continue;
      }
      throw new Error(`ASR killed by signal ${result.signal}: ${stderr}`);
    }

    if (result.error) throw new Error(`Python ASR subprocess failed: ${result.error.message}`);
    if (result.status !== 0) {
      const stderr = result.stderr?.toString() || "";
      throw new Error(`Python ASR exited with status ${result.status}: ${stderr}`);
    }

    const asrOutputPath = parseAsrOutput(result.stdout?.toString() || "");
    if (!asrOutputPath || !existsSync(asrOutputPath)) {
      throw new Error(`Python ASR did not produce output at ${asrOutputPath}`);
    }

    const asr = await readJson(asrOutputPath);
    const actualDevice = fallbackToCpu ? "cpu" : useGpu ? "cuda" : "cpu";
    if (asr.detected_language) {
      setCtx(taskDir, {
        asr_language: asr.detected_language,
        runInfo: {
          asr: {
            engine: "faster-whisper",
            device: actualDevice,
            computeType: actualDevice === "cpu" ? "int8" : "float16",
            gpuAttempted: useGpu,
            fallbackToCpu,
          },
        },
      });
    }
    emitAsrTiming(taskDir, asr, elapsedSec);

    return;
  }
}
