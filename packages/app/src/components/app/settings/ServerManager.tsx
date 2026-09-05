import { createQuery, useMutation, useQueryClient } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { CardX } from "@repo/ui-solid/custom/card";
import { toastError } from "@repo/ui-solid/custom/toast";
import { ModelServerStatus } from "@repo/core/servers/type";
import {
  checkMainServer,
  get_voxcpm_torch_gradio_status,
  restartVoxCpm,
  startVoxCpm,
  stopVoxCpm,
} from "#/feat/servers/servers.ts";
import { fnrpc } from "#/integrations/fnrpc/client.ts";
import { cn } from "@repo/shared/lib/utils";

function fmtUptime(s: number): string {
  if (!s) return "0s";
  const hh = Math.floor(s / 3600);
  const mm = Math.floor((s % 3600) / 60);
  const ss = Math.floor(s % 60);
  return `${hh}h ${mm}m ${ss}s`;
}

function statusDot(status: string): string {
  return cn("w-3 h-3 rounded-full shrink-0", {
    "bg-[#22c55e]": status === "running",
    "bg-[#ef4444]": status === "stopped" || status === "error",
    "bg-[#facc15]": status === "pending",
    "bg-gray-400": status === "unknown",
  });
}

function ServerCard(props: {
  name: string;
  running: boolean;
  uptimeS: number;
  port: number;
  models: Record<string, { status: string; device: string }>;
  busy: boolean;
  data?: ModelServerStatus;
  error?: Error | null;
  isLoading?: boolean;
  hideActions?: boolean;
  onStart?: () => void;
  onStop?: () => void;
  onRestart?: () => void;
}) {
  const isLoading = () => props.isLoading ?? false;
  const status = () => {
    if (isLoading()) return "pending";
    if (props.error) return "error";
    return props.data?.status ?? "unknown";
  };
  const statusText = () => {
    if (isLoading()) return "Loading...";
    if (props.error) return `Error: ${props.error.message}`;
    return props.data?.status ?? "unknown";
  };
  return (
    <CardX
      title={props.name}
      description={statusText()}
      Footer={
        <div class="flex w-full flex-col gap-3">
          <div class="flex items-center gap-3">
            <div class={statusDot(status())} />
            <span class="text-sm text-gray-500">
              {props.busy
                ? "working..."
                : props.running
                  ? `uptime ${fmtUptime(props.uptimeS)}`
                  : "stopped"}
            </span>
          </div>
          {props.running ? (
            <div class="text-xs text-gray-400">http://127.0.0.1:{props.port}</div>
          ) : null}
          <div class="flex flex-wrap gap-2">
            {Object.entries(props.models).map(([name, m]) => (
              <span
                class={`text-xs px-2 py-0.5 rounded ${
                  m.status === "ready"
                    ? "bg-green-900/40 text-green-400"
                    : "bg-gray-800 text-gray-500"
                }`}
              >
                {name}: {m.status}
                {m.device ? ` (${m.device})` : ""}
              </span>
            ))}
          </div>
          {props.hideActions ? null : (
            <div class="flex gap-2">
              {props.onStart ? (
                <Button
                  variant="ghost"
                  onClick={props.onStart}
                  disabled={props.busy || props.running}
                  class="font-medium bg-green-400 disabled:opacity-40"
                >
                  Start
                </Button>
              ) : null}
              {props.onRestart ? (
                <Button
                  onClick={props.onRestart}
                  disabled={props.busy || !props.running}
                  class="font-medium bg-amber-300 disabled:opacity-40"
                >
                  Restart
                </Button>
              ) : null}
              {props.onStop ? (
                <Button
                  onClick={props.onStop}
                  disabled={props.busy || !props.running}
                  class="font-medium bg-red-400 disabled:opacity-40"
                >
                  Stop
                </Button>
              ) : null}
            </div>
          )}
        </div>
      }
    />
  );
}

export function ServerManager() {
  const queryClient = useQueryClient();
  const mainServerHealth = createQuery(() => ({
    queryKey: ["mainServerHealth"],
    queryFn: checkMainServer,
    staleTime: 3000,
  }));

  const voxcpm_torch_gradio_status = createQuery(() => ({
    queryKey: ["voxcpm_torch_gradio_status"],
    queryFn: get_voxcpm_torch_gradio_status,
    staleTime: 3000,
  }));

  // Main Server 启停: 走 fnrpc (Tauri IPC), 复用 cli servers start 的同一份逻辑
  // (start_main_server: 幂等检测 + spawn detached + 日志落盘)。
  const startMain = useMutation(() => ({
    mutationFn: () => fnrpc.start_main(),
    onError: (e) => toastError(e),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["mainServerHealth"] }),
  }));
  const stopMain = useMutation(() => ({
    mutationFn: () => fnrpc.shutdown(),
    onError: (e) => toastError(e),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["mainServerHealth"] }),
  }));

  const startVox = useMutation(() => ({ mutationFn: startVoxCpm, onError: (e) => toastError(e) }));
  const stopVox = useMutation(() => ({ mutationFn: stopVoxCpm, onError: (e) => toastError(e) }));
  const restartVox = useMutation(() => ({
    mutationFn: restartVoxCpm,
    onError: (e) => toastError(e),
  }));

  const vcModels = () => {
    const m = voxcpm_torch_gradio_status.data?.models;
    if (!m) return { voxcpm: { status: "unloaded", device: "" } };
    return m;
  };

  return (
    <div>
      <h2>服务器</h2>
      <div class="space-y-4">
        <ServerCard
          name="Main Server"
          data={mainServerHealth.data}
          running={mainServerHealth.data?.status === "running"}
          uptimeS={mainServerHealth.data?.uptime_s ?? 0}
          port={mainServerHealth.data?.port ?? 19110}
          models={{}}
          busy={startMain.isPending || stopMain.isPending}
          onStart={() => startMain.mutate()}
          onStop={() => stopMain.mutate()}
        />
        <ServerCard
          name="VoxCPM PyTorch Server"
          data={voxcpm_torch_gradio_status.data}
          isLoading={voxcpm_torch_gradio_status.isLoading}
          error={voxcpm_torch_gradio_status.error}
          running={voxcpm_torch_gradio_status.data?.status === "running"}
          uptimeS={voxcpm_torch_gradio_status.data?.uptime_s ?? 0}
          port={voxcpm_torch_gradio_status.data?.port ?? 19112}
          models={vcModels()}
          busy={startVox.isPending || stopVox.isPending || restartVox.isPending}
          onStart={() => startVox.mutate()}
          onStop={() => stopVox.mutate()}
          onRestart={() => restartVox.mutate()}
        />
      </div>
    </div>
  );
}
