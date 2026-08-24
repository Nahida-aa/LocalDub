import { REPO_ROOT } from "@repo/config/root";
import { setCtx, setStage } from "@repo/core/context/context";
import { parseAsrOutput } from "@repo/core/stages/asr/utils";
import { existsSync, readJson } from "@repo/core/utils/fileOps";
import { emitAsrTiming } from "../time";
import { AsrOptions } from "@repo/core/stages/asr/types";
import path from "node:path";
import { spawnSync } from "node:child_process";

// Bun.pathToFileURL

async function asrPytorch(opts: AsrOptions) {
  let { taskId, audioPath, taskDir: taskDir, language, device, pythonBin: pyBin, ctx } = opts;
  const script = path.join(REPO_ROOT, "packages", "cli", "scripts", "asr", "pytorch.py");

  if (device === "cuda") {
    const cudaResult = spawnSync(pyBin, ["-c", "import torch; print(torch.cuda.is_available())"]);
    const cudaOk =
      cudaResult.status === 0 && (cudaResult.stdout?.toString().trim() ?? "") === "True";
    if (!cudaOk) {
      console.warn(`[WARN] [ASR] torch.cuda.is_available()=${cudaOk}, falling back to CPU`);
      device = "cpu";
    }
  }

  const attempts = device !== "cpu" ? 2 : 1;
  let fallbackToCpu = false;

  const emitWords = ctx.input?.stages?.asr?.wordsOutput ?? false;
  for (let attempt = 0; attempt < attempts; attempt++) {
    const actualDevice = attempt === 0 ? device : "cpu";
    const args = [script, audioPath, taskDir, language || "auto", "--device", actualDevice];
    if (emitWords) args.push("--word-timestamps");
    const t0 = Date.now();
    const result = spawnSync(pyBin, args, {
      maxBuffer: 256 * 1024 * 1024,
      timeout: 600_000,
    });
    const elapsedSec = (Date.now() - t0) / 1000;

    if (result.error || result.signal || result.status !== 0) {
      if (attempt === 0 && device !== "cpu") {
        const stderr = (result.stderr?.toString() || "").trim().slice(-200);
        console.warn(
          `[WARN] [ASR] GPU failed (${result.error?.message || `signal ${result.signal}` || `exit ${result.status}`}), retrying CPU: ${stderr}`,
        );
        await setStage(taskDir, "asr", {
          last_message: "GPU failed, retrying CPU...",
        });
        fallbackToCpu = true;
        continue;
      }
      if (result.error) throw new Error(`Python ASR subprocess failed: ${result.error.message}`);
      if (result.signal)
        throw new Error(
          `ASR killed by signal ${result.signal}: ${(result.stderr?.toString() || "").trim().slice(-200)}`,
        );
      throw new Error(
        `Python ASR exited with status ${result.status}: ${result.stderr?.toString() || ""}`,
      );
    }

    const asrOutputPath = parseAsrOutput(result.stdout?.toString() || "");
    if (!asrOutputPath || !existsSync(asrOutputPath)) {
      throw new Error(`Python ASR did not produce output at ${asrOutputPath}`);
    }

    const asr = await readJson(asrOutputPath);
    if (asr.detected_language) {
      setCtx(taskDir, {
        asr_language: asr.detected_language,
        runInfo: {
          asr: {
            engine: "whisper-pytorch",
            device: actualDevice,
            gpuAttempted: device !== "cpu",
            fallbackToCpu,
          },
        },
      });
    }
    emitAsrTiming(taskDir, asr, elapsedSec);
    return;
  }
}
