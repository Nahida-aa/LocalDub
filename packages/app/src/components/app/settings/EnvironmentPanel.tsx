import { createSignal, Show } from "solid-js";
import { createQuery, useMutation } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { toastError } from "@repo/ui-solid/custom/toast";
import { client } from "#/integrations/fnrpc/client.ts";
import { cn } from "@repo/shared/lib/utils";
import type { EnvCheckItem } from "@repo/sdk/fnrpc/bindings";

const statusColor = (status: string) =>
  cn("text-xs font-medium", {
    "text-[#22c55e]": status === "pass",
    "text-[#facc15]": status === "warn",
    "text-[#ef4444]": status === "fail",
    "text-gray-500": status === "skip",
  });

function dataSummary(item: EnvCheckItem): string {
  const d = item.data as Record<string, unknown>;
  if (typeof d?.msg === "string" && d.msg) return d.msg;
  if (typeOfString(d?.missing)) return `missing: ${d.missing}`;
  if (typeof d?.version === "string" && d.version) return `version ${d.version}`;
  return "";
}

function typeOfString(v: unknown): v is string {
  return typeof v === "string";
}

const sortWeight = (status: string) =>
  status === "fail" ? 0 : status === "warn" ? 1 : status === "pass" ? 2 : 3;

export function EnvironmentPanel() {
  const [scope, setScope] = createSignal<"infer" | "all">("infer");
  const q = createQuery(() => client.env_check.queryOptions(scope() === "infer" ? [] : ["*"]));
  const install = useMutation(() => ({
    mutationFn: (keys: string[]) => client.env_ensure.mutationOptions().mutationFn(keys),
    onError: (e) => toastError(e),
  }));

  const items = () =>
    (q.data ?? []).sort((a, b) => {
      const req = Number(b.required) - Number(a.required);
      return req !== 0 ? req : sortWeight(a.status) - sortWeight(b.status);
    });

  const doInstall = async (key: string) => {
    await install.mutateAsync([key]);
    q.refetch();
  };

  return (
    <div class="space-y-4">
      <div class="flex items-center gap-2">
        <div class="flex rounded-lg border border-gray-700 p-0.5 text-sm">
          <button
            type="button"
            class={cn("rounded px-3 py-1", scope() === "infer" && "bg-gray-700 text-white")}
            onClick={() => setScope("infer")}
          >
            当前配置推断
          </button>
          <button
            type="button"
            class={cn("rounded px-3 py-1", scope() === "all" && "bg-gray-700 text-white")}
            onClick={() => setScope("all")}
          >
            全部
          </button>
        </div>
        <Button size="sm" variant="outline" onClick={() => q.refetch()} disabled={q.isFetching}>
          {q.isFetching ? "检查中..." : "重新检查"}
        </Button>
      </div>

      {q.isLoading && <p class="text-sm text-gray-500">检查中...</p>}
      {q.error && <p class="text-sm text-red-400">检查失败: {q.error.message}</p>}

      <Show when={q.data}>
        <div class="space-y-2">
          {items().map((item) => (
            <div class="flex items-start justify-between gap-3 rounded-lg border border-gray-700 p-3 text-sm">
              <div class="min-w-0 space-y-0.5">
                <div class="flex items-center gap-2">
                  <span class="font-mono text-[13px]">{item.key}</span>
                  <span class={statusColor(item.status)}>{item.status}</span>
                  {item.required && (
                    <span class="text-[11px] text-[#facc15] border border-[#facc15]/40 rounded px-1">
                      必需
                    </span>
                  )}
                  <span class="text-[11px] text-gray-500">{item.category}</span>
                </div>
                <div class="text-gray-400">{item.zh}</div>
                {item.en && item.en !== item.zh && (
                  <div class="truncate text-xs text-gray-600">{item.en}</div>
                )}
                <div class="truncate text-xs text-gray-500">{dataSummary(item)}</div>
              </div>
              {item.has_ensure && (
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={install.isPending}
                  onClick={() => doInstall(item.key)}
                >
                  {install.isPending ? "安装中..." : "安装/更新"}
                </Button>
              )}
            </div>
          ))}
        </div>
      </Show>
    </div>
  );
}
