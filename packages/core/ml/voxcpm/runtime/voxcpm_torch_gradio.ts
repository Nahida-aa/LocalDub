import { VoxCPMCloud, type TTSBackend } from "@repo/voxlab";
import { join } from "node:path";
import type { ModelServerStatus } from "@repo/core/servers/type";
import { spawn, type ChildProcess } from "node:child_process";
import { to } from "@repo/shared/lib/utils/try";
import { fetchStatsData } from "@repo/core/servers/client";
import { TtsArgs } from "../../../stages/07_tts/args";
import { pythonBin, VOXCPM_TORCH_GRADIO_MAIN } from "@repo/config/path/bin";
import { findServer, readPortFromOutput } from "@repo/core/servers/discovery";

export const voxcpmTorchGradioStatus = async ({
  port,
}: {
  port: number;
}): Promise<ModelServerStatus> => {
  const [data, err] = await to(fetchStatsData(port));
  if (err)
    return {
      status: "stopped",
      port,
      uptime_s: 0,
      message: "Not running",
      models: { voxcpm: { status: "unloaded", device: "" } },
    };
  return data;
};

async function waitForHealth(port: number, timeoutMs = 120_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 500));
    const [data] = await to(fetchStatsData(port));
    if (data?.status === "running") return;
  }
  throw new Error(`VoxCPM Gradio server startup timeout after ${timeoutMs}ms`);
}

export const startVoxCPMTorchGradioServer = async ({
  port: hintPort,
  device = "cpu",
  modelDir,
  healthPolling = false,
}: {
  port?: number;
  device?: string;
  modelDir?: string;
  healthPolling?: boolean;
}): Promise<{
  url: string;
  proc?: ChildProcess;
}> => {
  // 1) Check if already running
  const hint = hintPort ?? 19112;
  const { port } = await findServer("voxcpm_torch_gradio");
  {
    const [data] = await to(fetchStatsData(port));
    if (data?.status === "running") {
      return { url: `http://127.0.0.1:${port}` };
    }
  }

  // 2) Spawn
  const args = [VOXCPM_TORCH_GRADIO_MAIN, "--port", String(hint), "--device", device];
  if (modelDir) args.push("--model-dir", modelDir);
  const proc = spawn(pythonBin(), args, {
    env: { ...(process.env as Record<string, string>) },
    stdio: ["ignore", "pipe", "pipe"],
  });

  // 3) Read PORT=N from stdout
  let stdout = "";
  proc.stdout?.on("data", (d: Buffer) => {
    stdout += d.toString();
  });
  proc.stderr?.pipe(process.stderr);

  const actualPort = await new Promise<number>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("port discovery timeout")), 10000);
    const iv = setInterval(() => {
      const p = readPortFromOutput(stdout);
      if (p) {
        clearTimeout(timer);
        clearInterval(iv);
        resolve(p);
      }
    }, 100);
  });

  // 4) Health polling
  if (healthPolling) await waitForHealth(actualPort);

  return { url: `http://127.0.0.1:${actualPort}`, proc };
};

export const stopVoxCPMTorchGradioServer = async ({ port }: { port: number }): Promise<void> => {
  const res = await fetch(`http://127.0.0.1:${port}/shutdown`, {
    method: "POST",
    signal: AbortSignal.timeout(2000),
  });
  if (!res.ok) throw new Error(`VoxCPM Gradio shutdown failed: ${res.status}`);
};

export const loadVoxCPMTorchGradioModel = async (
  baseUrl: string,
  device: string,
): Promise<void> => {
  const res = await fetch(`${baseUrl}/load-model`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ device }),
  });
  if (!res.ok) throw new Error(`load-model failed: ${res.status}`);
};

export const connectToVoxCPMTorchGradioServer = async ({
  baseUrl,
  device = "cpu",
}: {
  baseUrl: string;
  device: string;
}) => {
  const port = Number(new URL(baseUrl).port);
  const res = await voxcpmTorchGradioStatus({ port });

  if (res.status === "running") {
    if (res.models.voxcpm.status === "ready") {
      return { url: baseUrl, proc: undefined };
    } else if (res.models.voxcpm.status === "unloaded") {
      await loadVoxCPMTorchGradioModel(baseUrl, device);
      return { url: baseUrl, proc: undefined };
    } else if (res.models.voxcpm.status === "loading") {
    } else if (res.models.voxcpm.status === "error") {
    }
  } else if (res.status === "stopped") {
    const ret = await startVoxCPMTorchGradioServer({ port, device });
    await loadVoxCPMTorchGradioModel(ret.url, device);
    return ret;
  } else if (res.status === "error") {
  }
};

export const newVoxCPMPyTorchGradio = (cfg: TtsArgs): TTSBackend => {
  let cloud: VoxCPMCloud | null = null;
  let device = cfg && "device" in cfg ? (cfg.device as string) : "cpu";
  let port = 19112;
  return {
    name: "voxcpm_torch_gradio",
    async load(): Promise<void> {
      console.log(`[VoxCPM] connecting to Gradio server on port ${port}...`);

      const ret = await connectToVoxCPMTorchGradioServer({
        baseUrl: `http://127.0.0.1:${port}`,
        device,
      });

      cloud = new VoxCPMCloud({ apiUrl: ret?.url });
      await cloud.load();
    },
    async generate(
      options: Parameters<TTSBackend["generate"]>[0],
    ): ReturnType<TTSBackend["generate"]> {
      if (!cloud) throw new Error("VoxCPMGradio not loaded");
      return cloud.generate(options);
    },
    async dispose(): Promise<void> {
      await cloud?.dispose();
    },
  };
};
