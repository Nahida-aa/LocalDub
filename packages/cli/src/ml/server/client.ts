import { type ChildProcess, spawn } from "node:child_process";
import { delimiter, join } from "node:path";
import { ModelServerStatus } from "@repo/core/servers/type";
import { pythonBin } from "@repo/config/path/bin";
import { demucs_torch_server } from "@repo/config/path/scripts";
import { REPO_ROOT } from "@repo/config/root";

export const fetchStatsRes = (port: number) =>
  fetch(`http://127.0.0.1:${port}/status`, {
    signal: AbortSignal.timeout(2000),
  });

export const fetchStatsData = async (port: number): Promise<ModelServerStatus> => {
  const res = await fetchStatsRes(port);
  if (!res.ok) throw new Error(`Failed to fetch status from port ${port}: ${res.status}`);
  return (await res.json()) as ModelServerStatus;
};

const DEFAULT_PORT = 19109;
let _torchServerUrl = "";
let _serverProc: ChildProcess | null = null;

type ProgressCallback = (current: number, total: number, message?: string) => void;
type LogCallback = (line: string) => void;

export const getTorchServerUrl = (port: number) => `http://127.0.0.1:${port}`;

function readSSE(
  stream: ReadableStream<Uint8Array>,
  onProgress?: ProgressCallback,
  onLog?: LogCallback,
): Promise<{ ok: true; output: Record<string, unknown> } | { ok: false; message: string }> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let currentEvent = "";
  let currentData = "";

  function dispatch() {
    if (currentEvent === "progress") {
      const d = JSON.parse(currentData);
      onProgress?.(d.current, d.total, d.message);
      return null;
    }
    if (currentEvent === "complete") {
      return { ok: true as const, output: JSON.parse(currentData).output ?? {} };
    }
    if (currentEvent === "error") {
      return { ok: false as const, message: JSON.parse(currentData).message ?? "Unknown error" };
    }
    if (currentEvent === "log") {
      const d = JSON.parse(currentData) as { line?: string };
      onLog?.(d.line ?? "");
      return null;
    }
    return null;
  }

  return new Promise((resolve, reject) => {
    function pump() {
      reader
        .read()
        .then(({ done, value }) => {
          if (done) {
            resolve({ ok: false, message: "SSE stream ended without complete/error" });
            return;
          }
          buffer += decoder.decode(value, { stream: true });
          const parts = buffer.split("\n");
          buffer = parts.pop() ?? "";
          for (const line of parts) {
            if (line.startsWith("event: ")) {
              currentEvent = line.slice(7).trim();
            } else if (line.startsWith("data: ")) {
              currentData = line.slice(6).trim();
            } else if (line === "" && currentEvent) {
              const result = dispatch();
              if (result) {
                resolve(result);
                reader.cancel();
                return;
              }
              currentEvent = "";
              currentData = "";
            }
          }
          pump();
        })
        .catch(reject);
    }
    pump();
  });
}

async function healthCheck(baseUrl: string): Promise<boolean> {
  try {
    const res = await fetch(`${baseUrl}/status`, { signal: AbortSignal.timeout(2000) });
    return res.ok;
  } catch {
    return false;
  }
}

export async function startTorchServer(preferredPort: number = DEFAULT_PORT): Promise<string> {
  // torch 服务器固定端口 (不再 mDNS 发现 demucs_torch_server)
  const port = preferredPort;
  const baseUrl = getTorchServerUrl(port);
  if (await healthCheck(baseUrl)) {
    console.log(`[TorchServer] Connected to existing server at ${baseUrl}`);
    _torchServerUrl = baseUrl;
    return baseUrl;
  }

  // 2) Spawn detached Torch server
  console.log("[TorchServer] Spawning ML torch server...");
  const voxcpmSrc = join(REPO_ROOT, "submodule", "VoxCPM", "src");

  const env: Record<string, string> = {
    ...(process.env as Record<string, string>),
    TORCHAUDIO_USE_BACKEND: "soundfile",
  };
  const existingPy = env.PYTHONPATH || "";
  env.PYTHONPATH = existingPy ? `${voxcpmSrc}${delimiter}${existingPy}` : voxcpmSrc;

  const proc = spawn(pythonBin(), [demucs_torch_server, "--http-port", String(port)], {
    env,
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  proc.stderr?.pipe(process.stderr);
  proc.unref();
  _serverProc = proc;

  // 3) Poll health endpoint until ready
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 200));
    if (await healthCheck(baseUrl)) {
      console.log(`[TorchServer] Server ready at ${baseUrl} (pid ${proc.pid})`);
      _torchServerUrl = baseUrl;
      return baseUrl;
    }
  }

  throw new Error(`TorchServer startup timeout after 60000ms`);
}

export async function stopTorchServer(baseUrl: string): Promise<void> {
  try {
    await fetch(`${baseUrl}/api/shutdown`, { method: "POST" });
  } catch {
    // already gone
  }
  if (_serverProc) {
    _serverProc.stdout?.destroy();
    _serverProc.stderr?.destroy();
    _serverProc = null;
  }
}

export async function runStage(
  baseUrl: string,
  stage: string,
  taskId: string,
  params: Record<string, unknown>,
  onProgress?: ProgressCallback,
  onLog?: LogCallback,
): Promise<Record<string, unknown>> {
  const res = await fetch(`${baseUrl}/api/run/${stage}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ task_id: taskId, params }),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`TorchServer HTTP ${res.status}: ${text}`);
  }
  if (!res.body) throw new Error("TorchServer returned empty response body");

  const result = await readSSE(res.body, onProgress, onLog);
  if (result.ok) return result.output;
  throw new Error(result.message);
}
