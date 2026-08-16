import { createSignal, onCleanup, onMount } from "solid-js";
import { createQuery, useMutation } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { CardX } from "@repo/ui-solid/custom/card";
import { toastError } from "@repo/ui-solid/custom/toast";
import { ModelServerStatus } from "@repo/core/servers/type";
import {
  checkMainServer,
  checkTorch,
  get_voxcpm_torch_gradio_status,
  restartTorch,
  restartVoxCpm,
  startTorch,
  startVoxCpm,
  stopTorch,
  stopVoxCpm,
} from "#/feat/servers/servers.ts";
import { cn } from "@repo/shared/lib/utils";

function fmtUptime(s: number): string {
  if (!s) return "0s";
  const hh = Math.floor(s / 3600);
  const mm = Math.floor((s % 3600) / 60);
  const ss = Math.floor(s % 60);
  return `${hh}h ${mm}m ${ss}s`;
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
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
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
    <div class="space-y-3 p-4 border rounded-lg">
      <div class="flex items-center gap-3">
        <div
          class={cn("w-3 h-3 rounded-full shrink-0", {
            "bg-[#22c55e]": status() === "running",
            "bg-[#ef4444]": status() === "stopped" || status() === "error",
            "bg-[#facc15]": status() === "pending",
            "bg-gray-400": status() === "unknown",
          })}
        />
        <span>{statusText()}</span>
        <span class="font-medium">{props.name}</span>
        <span class="text-sm text-gray-500 ">
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

      {props.hideActions ? null : (
        <div class="flex gap-2">
          <Button
            variant="ghost"
            onClick={props.onStart}
            disabled={props.busy || props.running}
            class="font-medium bg-green-400 disabled:opacity-40"
          >
            Start
          </Button>
          <Button
            onClick={props.onRestart}
            disabled={props.busy || !props.running}
            class="font-medium bg-amber-300 disabled:opacity-40"
          >
            Restart
          </Button>
          <Button
            onClick={props.onStop}
            disabled={props.busy || !props.running}
            class="font-medium bg-red-400 disabled:opacity-40"
          >
            Stop
          </Button>
        </div>
      )}

      <div class="flex flex-wrap gap-2">
        {Object.entries(props.data?.models ?? {}).map(([name, m]) => (
          <span
            class={`text-xs px-2 py-0.5 rounded ${
              m.status === "ready" ? "bg-green-900/40 text-green-400" : "bg-gray-800 text-gray-500"
            }`}
          >
            {name}: {m.status}
            {m.device ? ` (${m.device})` : ""}
          </span>
        ))}
      </div>
    </div>
  );
}

export function ServerManager() {
  const mainServerHealth = createQuery(() => ({
    queryKey: ["mainServerHealth"],
    queryFn: checkMainServer,
    staleTime: 3000,
  }));

  const torchHealth = createQuery(() => ({
    queryKey: ["torchHealth"],
    queryFn: checkTorch,
    staleTime: 3000,
  }));

  const voxcpm_torch_gradio_status = createQuery(() => ({
    queryKey: ["voxcpm_torch_gradio_status"],
    queryFn: get_voxcpm_torch_gradio_status,
    staleTime: 3000,
  }));

  voxcpm_torch_gradio_status.status;
  const startTorchM = useMutation(() => ({
    mutationFn: startTorch,
    onError: (e) => toastError(e),
  }));
  const stopTorchM = useMutation(() => ({ mutationFn: stopTorch, onError: (e) => toastError(e) }));
  const restartTorchM = useMutation(() => ({
    mutationFn: restartTorch,
    onError: (e) => toastError(e),
  }));

  const startVox = useMutation(() => ({ mutationFn: startVoxCpm, onError: (e) => toastError(e) }));
  const stopVox = useMutation(() => ({ mutationFn: stopVoxCpm, onError: (e) => toastError(e) }));
  const restartVox = useMutation(() => ({
    mutationFn: restartVoxCpm,
    onError: (e) => toastError(e),
  }));

  const torchModels = () => {
    const m = torchHealth.data?.models;
    if (!m || Object.keys(m).length === 0)
      return {
        asr: { status: "unloaded", device: "" },
        separate: { status: "unloaded", device: "" },
      };
    return m;
  };

  const vcModels = () => {
    const m = voxcpm_torch_gradio_status.data?.models;
    if (!m) return { voxcpm: { status: "unloaded", device: "" } };
    return m;
  };

  return (
    <div class="space-y-4">
      <ServerCard
        name="Main Server"
        data={mainServerHealth.data}
        running={mainServerHealth.data?.status === "running"}
        uptimeS={mainServerHealth.data?.uptime_s ?? 0}
        port={mainServerHealth.data?.port ?? 19110}
        models={{}}
        busy={false}
        hideActions
        onStart={() => {}}
        onStop={() => {}}
        onRestart={() => {}}
      />
      <ServerCard
        name="Torch Server"
        data={torchHealth.data}
        running={torchHealth.data?.status === "running"}
        uptimeS={torchHealth.data?.uptime_s ?? 0}
        port={torchHealth.data?.port ?? 19109}
        models={torchModels()}
        busy={startTorchM.isPending || stopTorchM.isPending || restartTorchM.isPending}
        onStart={() => startTorchM.mutate()}
        onStop={() => stopTorchM.mutate()}
        onRestart={() => restartTorchM.mutate()}
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
  );
}
