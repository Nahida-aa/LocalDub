import { to } from "@repo/shared/lib/utils/try";
// import { findServer } from '@repo/core/servers/discovery'
import type { ModelServerStatus } from "@repo/core/servers/type";
import { fetchStatsRes } from "@repo/core/servers/client";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
// import { client } from '#/integrations/rspc/rspc.ts';

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

// 主服务器 (packages/server) 管理。主服务器是 fnrpc 载体, 由 app 生命周期启动,
// 设置界面仅显示状态 (停止会导致 UI 失联, 不做启停)。

const MAIN_SERVER_PORT = 19110;

/** 探测主服务器状态 (GET /fnrpc/health_check, 返回 "ok" = running)。 */
export async function checkMainServer(): Promise<ModelServerStatus> {
  try {
    const res = await fetch(`http://127.0.0.1:${MAIN_SERVER_PORT}/fnrpc/health_check`, {
      signal: AbortSignal.timeout(2000),
    });
    if (!res.ok) return { status: "stopped", port: MAIN_SERVER_PORT, uptime_s: 0, models: {} };
    const data = (await res.json()) as { json?: string };
    return {
      status: data?.json === "ok" ? "running" : "stopped",
      port: MAIN_SERVER_PORT,
      uptime_s: 0,
      models: {},
    };
  } catch {
    return { status: "stopped", port: MAIN_SERVER_PORT, uptime_s: 0, models: {} };
  }
}
