import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import { Card, CardContent, CardHeader, CardTitle } from "@repo/ui-solid/base/card";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { Link } from "@tanstack/solid-router";
import { CardX } from "@repo/ui-solid/custom/card";
import { ScrollArea } from "@repo/ui-solid/base/scroll-area";
import { Separator } from "@repo/ui-solid/base/separator";
import { consumeEventIterator } from "@fnrpc/client";
import { createQuery } from "@tanstack/solid-query";
import { GroupInfo, WorkflowBrief } from "@repo/sdk/index";

function StatCard(p: { label: string; value: number }) {
  return <CardX title={p.label} Footer={<span class="text-2xl font-semibold">{p.value}</span>} />;
}

function StatusBadge(p: { status: string }) {
  const cls = (() => {
    switch (p.status) {
      case "completed":
        return "bg-green-600/20 text-green-500";
      case "running":
        return "bg-blue-600/20 text-blue-400";
      case "failed":
        return "bg-red-600/20 text-red-400";
      case "queued":
        return "bg-yellow-600/20 text-yellow-400";
      default:
        return "bg-muted text-muted-foreground";
    }
  })();
  return <span class={`text-xs px-2 py-0.5 rounded-full ${cls}`}>{p.status}</span>;
}

function timeAgo(iso: string | null | undefined) {
  if (!iso) return "";
  const diff = Date.now() - new Date(iso).getTime();
  const min = Math.floor(diff / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

export function IndexPage() {
  import("@fnrpc/client").then(({ consumeEventIterator }) => {
    const stream = fnrpc.watch_workflow_log(
      "workfolder/深宫团宠，猫狗皇子皆是我的心头崽（30集）/第1集",
    );
    consumeEventIterator(stream, {
      onEvent: (line) => console.log("[log]", line),
      onError: (err) => console.error(err),
    });
  });
  const groupListQ = createQuery(() => client.get_group_list.queryOptions(null));
  const groups = () => (groupListQ.data ?? []) as GroupInfo[];
  const allVideos = () => groups().flatMap((g) => g.workflows);
  const s = () => {
    const t = allVideos();
    return {
      groups: groups().length,
      total: t.length,
      running: t.filter((x) => x.status === "running").length,
      failed: t.filter((x) => x.status === "failed").length,
    };
  };
  const recentVideos = () =>
    allVideos()
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
      .slice(0, 20);

  return (
    <div class="flex flex-col gap-6 p-6 h-full overflow-auto">
      <h1 class="text-lg font-semibold">Dashboard</h1>
      <Show when={groupListQ.isPending}>
        <span class="text-muted-foreground">Loading...</span>
      </Show>
      <Show when={groupListQ.isError}>
        <span class="text-red-400">Error: {groupListQ.error?.message}</span>
      </Show>
      <Show when={groupListQ.isSuccess}>
        <div class="grid grid-cols-4 gap-4">
          <StatCard label="Groups" value={s().groups} />
          <StatCard label="Videos" value={s().total} />
          <StatCard label="Running" value={s().running} />
          <StatCard label="Failed" value={s().failed} />
        </div>
        <CardX
          title="Recent Videos"
          class="gap-0"
          Footer={
            <div class="w-full">
              <div class="grid grid-cols-[auto_1fr_auto_auto] gap-x-4 gap-y-1 px-4 py-2 text-xs text-muted-foreground ">
                <span>Status</span>
                <span>Workflow</span>
                <span>Stage</span>
                <span>Time</span>
              </div>
              <Separator orientation="horizontal" class="mx-2" />
              <ScrollArea scrollbarSize={10}>
                <For each={recentVideos()}>
                  {(workflow: WorkflowBrief) => {
                    const group = groups().find((g) =>
                      g.workflows.some((t) => t.id === workflow.id),
                    );
                    return (
                      <Link
                        to={`/group/${group?.group_id || "unknown"}/${workflow.id}` as any}
                        class="grid grid-cols-[auto_1fr_auto_auto] gap-x-4 gap-y-1 px-4 py-2 text-sm hover:bg-accent/30 items-center"
                      >
                        <StatusBadge status={workflow.status} />
                        <span class="truncate">{workflow.id}</span>
                        <span class="text-xs text-muted-foreground">
                          {workflow.current_stage || "—"}
                        </span>
                        <span class="text-xs text-muted-foreground">
                          {timeAgo(workflow.completed_at || workflow.created_at)}
                        </span>
                      </Link>
                    );
                  }}
                </For>
              </ScrollArea>
            </div>
          }
          FooterClass="p-0"
        />
      </Show>
    </div>
  );
}
