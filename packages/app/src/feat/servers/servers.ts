import { to } from "@repo/shared/lib/utils/try";
// import { findServer } from '@repo/core/servers/discovery'
import type { ModelServerStatus } from "@repo/core/servers/type";
import { fetchStatsRes } from "@repo/core/servers/client";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
// import { client } from '#/integrations/rspc/rspc.ts';

let _torchPort = 19109;
let _voxcpmPort = 19112;

async function fetchStats(port: number): Promise<ModelServerStatus> {
  console.log(`fetchStats(${port})`);
  const [res, err] = await to(fetchStatsRes(port));
  if (err) return { status: "stopped", port, uptime_s: 0, models: {} };
  if (!res.ok) return { status: "stopped", port, uptime_s: 0, models: {} };
  const data = (await res.json()) as ModelServerStatus;
  console.log(`fetchStats(${port}) =>`, data);
  return data;
}

async function ping(port: number): Promise<boolean> {
  const [res, err] = await to(fetchStatsRes(port));
  if (err) return false;
  return res.ok;
}

async function waitForHealth(port: number, timeoutMs = 60_000): Promise<ModelServerStatus> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 200));
    const status = await fetchStats(port);
    if (status.status === "running") return status;
  }
  throw new Error(`TorchServer startup timeout after ${timeoutMs}ms`);
}

async function waitForVoxCpm(port: number, timeoutMs = 120_000): Promise<ModelServerStatus> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 200));
    const status = await fetchStats(port);
    if (status.status === "running") return status;
  }
  return {
    status: "stopped",
    port,
    uptime_s: 0,
    models: { voxcpm: { status: "error", device: "" } },
  };
}

export async function startTorch(): Promise<ModelServerStatus> {
  const { port } = await fnrpc.find_server("demucs_torch_server");
  _torchPort = port;
  if (await ping(port)) return fetchStats(port);

  _torchPort = await fnrpc.start_torch();
  return waitForHealth(_torchPort);
}

export async function stopTorch(): Promise<ModelServerStatus> {
  try {
    await fetch(`http://127.0.0.1:${_torchPort}/api/shutdown`, { method: "POST" });
  } catch {
    // already gone
  }
  await fnrpc.start_torch();
  return { status: "stopped", port: _torchPort, uptime_s: 0, models: {} };
}

export async function restartTorch(): Promise<ModelServerStatus> {
  try {
    await fetch(`http://127.0.0.1:${_torchPort}/api/shutdown`, { method: "POST" });
  } catch {
    /* ok */
  }
  await fnrpc.start_torch();

  await new Promise((r) => setTimeout(r, 1500));

  _torchPort = await fnrpc.start_torch();
  return waitForHealth(_torchPort);
}

export async function checkTorch(): Promise<ModelServerStatus> {
  const { port } = await fnrpc.find_server("demucs_torch_server");
  _torchPort = port;
  return fetchStats(port);
}

// VoxCPM server management

async function fetchVoxCpmHealth(port: number): Promise<ModelServerStatus> {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/status`, {
      signal: AbortSignal.timeout(2000),
    });
    if (!res.ok)
      return {
        status: "stopped",
        port,
        uptime_s: 0,
        models: { voxcpm: { status: "unloaded", device: "" } },
      };
    return (await res.json()) as ModelServerStatus;
  } catch {
    return {
      status: "stopped",
      port,
      uptime_s: 0,
      models: { voxcpm: { status: "unloaded", device: "" } },
    };
  }
}

async function pingVoxCpm(port: number): Promise<boolean> {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/status`, {
      signal: AbortSignal.timeout(2000),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export async function startVoxCpm(): Promise<ModelServerStatus> {
  const { port } = await fnrpc.find_server("voxcpm_torch_gradio");
  _voxcpmPort = port;
  if (await pingVoxCpm(port)) return fetchVoxCpmHealth(port);

  _voxcpmPort = await fnrpc.start_voxcpm();
  return waitForVoxCpm(_voxcpmPort);
}

export async function get_voxcpm_torch_gradio_status(): Promise<ModelServerStatus> {
  console.log(`get_voxcpm_torch_gradio_status(), _voxcpmPort=${_voxcpmPort}`);
  const { port } = await fnrpc.find_server("voxcpm_torch_gradio");
  console.log(`get_voxcpm_torch_gradio_status() => found port=${port}`);
  _voxcpmPort = port;
  return fetchStats(port);
}

export async function stopVoxCpm(): Promise<ModelServerStatus> {
  await fnrpc.stop_voxcpm();
  return {
    status: "stopped",
    port: _voxcpmPort,
    uptime_s: 0,
    models: { voxcpm: { status: "unloaded", device: "" } },
  };
}

export async function restartVoxCpm(): Promise<ModelServerStatus> {
  await stopVoxCpm();
  await new Promise((r) => setTimeout(r, 1500));
  return startVoxCpm();
}
