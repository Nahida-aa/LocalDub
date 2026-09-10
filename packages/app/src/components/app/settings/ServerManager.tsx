import { createQuery, useMutation, useQueryClient } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { CardX } from "@repo/ui-solid/custom/card";
import { toastError } from "@repo/ui-solid/custom/toast";
import { fnrpc } from "#/integrations/fnrpc/client.ts";
import { cn } from "@repo/shared/lib/utils";
import type { ModelServerStatus, ModelStatus } from "@repo/sdk/index";

function fmtUptime(s?: number | bigint): string {
  if (s == null) return "0s";
  const total = BigInt(s);
  const hh = total / 3600n;
  const mm = (total % 3600n) / 60n;
  const ss = total % 60n;
  return `${hh}h ${mm}m ${ss}s`;
}

function statusDot(status: string): string {
  return cn("w-3 h-3 rounded-full shrink-0", {
    "bg-[#22c55e]": status === "running",
    "bg-[#ef4444]": status === "stopped" || status === "error",
    "bg-[#facc15]": status === "pending" || status === "timeout",
    "bg-gray-400": status === "unknown",
  });
}

function ServerCard(props: {
  name: string;
  running: boolean;
  port: number;
  models: Record<string, ModelStatus>;
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
    if (props.data?.message && props.data.status !== "running") {
      return props.data.message;
    }
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
                  ? `uptime ${fmtUptime(props.data?.uptime_s)}`
                  : "stopped"}
            </span>
          </div>
          {props.running ? (
            <div class="text-xs text-gray-400">
              http://{props.data?.host ?? "127.0.0.1"}:{props.port}
            </div>
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
  const voxStatusKey = ["voxcpm_torch_gradio_status"] as const;

  // 主服务器是 fnrpc 载体, 停止会导致 UI 失联, 只展示状态不做启停 (由 app 生命周期启动)。
  const mainServerStatus = createQuery(() => ({
    queryKey: ["mainServerStatus"],
    queryFn: () => fnrpc.get_server_status("main"),
    staleTime: 3000,
  }));

  const voxcpm_torch_gradio_status = createQuery(() => ({
    queryKey: voxStatusKey,
    queryFn: () => fnrpc.get_server_status("voxcpm_torch_gradio"),
    staleTime: 3000,
  }));

  const startVox = useMutation(() => ({
    mutationFn: () => fnrpc.start_voxcpm(),
    onError: (e) => toastError(e),
    onSettled: () => queryClient.invalidateQueries({ queryKey: voxStatusKey }),
  }));
  const stopVox = useMutation(() => ({
    mutationFn: () => fnrpc.stop_voxcpm(),
    onError: (e) => toastError(e),
    onSettled: () => queryClient.invalidateQueries({ queryKey: voxStatusKey }),
  }));
  const restartVox = useMutation(() => ({
    mutationFn: async () => {
      await fnrpc.stop_voxcpm();
      await fnrpc.start_voxcpm();
    },
    onError: (e) => toastError(e),
    onSettled: () => queryClient.invalidateQueries({ queryKey: voxStatusKey }),
  }));

  const vcModels = () => voxcpm_torch_gradio_status.data?.models ?? {};

  return (
    <div>
      <h2>服务器</h2>
      <div class="space-y-4">
        <ServerCard
          name="Main Server"
          data={mainServerStatus.data}
          running={mainServerStatus.data?.status === "running"}
          port={mainServerStatus.data?.port ?? 19110}
          models={{}}
          busy={false}
        />
        <ServerCard
          name="VoxCPM PyTorch Server"
          data={voxcpm_torch_gradio_status.data}
          isLoading={voxcpm_torch_gradio_status.isLoading}
          error={voxcpm_torch_gradio_status.error}
          running={voxcpm_torch_gradio_status.data?.status === "running"}
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
