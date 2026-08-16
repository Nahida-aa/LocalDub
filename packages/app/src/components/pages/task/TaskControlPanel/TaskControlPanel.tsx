import { useParams } from "@tanstack/solid-router";
import { For, Show, createSignal } from "solid-js";
import { FileTree } from "./FileTree";
import { Play } from "lucide-solid";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import {
  set_resumeFrom,
  setRunningStage,
  setViewingTab,
  StageTab,
  use_resumeFrom,
  useRunningStage,
  useViewingTab,
} from "./taskControlPanelStore";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@repo/ui-solid/base/tabs";
import { stages_to_map } from "@repo/core/stages/utils/filtering";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { StageStatusBadge } from "./StageStatusBadge";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { TaskCtx } from "@repo/sdk/index";
import { StageName } from "@repo/core/tasks/args";

export const TaskControlPanel = (p: {
  ctx: TaskCtx;
  // onResumeFrom: (stageName: string | null) => void;
}) => {
  const params = useParams({ from: "/group/$id/$taskId" });
  const taskDir = `workfolder/${params().id}/${p.ctx.task.id}`;
  const stages = () => p.ctx.stages ?? [];
  const stage_map = () => stages_to_map(stages() ?? []);
  const tabs = () => ["root", ...stages().map((s) => s.name)] as StageTab[];
  const resumeFrom = use_resumeFrom();
  const runningStage = useRunningStage();
  const viewingTab = useViewingTab();
  const qc = useQueryClient();

  /**
   * 跳转到对应阶段前一个 tab 让用户确认\
   * 然后在内容界面点击运行按钮才会真的继续运行
   */
  const handleResumeFrom = (stageName?: StageTab | null) => {
    const allTabs = tabs();
    const idx = allTabs.indexOf(stageName ?? "root");
    if (idx > 0) {
      setViewingTab(allTabs[idx - 1]);
    }
    setRunningStage(stageName); // 高亮三角所在的当前 tab
    set_resumeFrom(stageName === "root" ? null : stageName);
  };

  const resume_task = useMutation(() =>
    client.continue_task.mutationOptions({
      onSuccess: () => {
        console.log("[continue] 继续运行 完成");
        // 运行结束后立即刷新 ctx 与文件树（watch 事件通常已覆盖，这里兜底）
        qc.invalidateQueries({
          queryKey: client.get_task_ctx.queryKey(taskDir),
        });
        qc.invalidateQueries({
          queryKey: client.list_app_directory.queryKey(taskDir),
        });
      },
      onError: (error) => {
        console.error("[continue] 继续运行 失败:", error);
      },
    }),
  );
  const handleConfirmResume = () => {
    const stage = resumeFrom();
    if (!stage) return;
    resume_task.mutate([taskDir, stage]);
    set_resumeFrom(null);
    setViewingTab(runningStage());
  };

  return (
    <div class="w-100 min-w-40 border-r flex text-muted-foreground text-sm overflow-hidden">
      <Tabs
        value={viewingTab()}
        onChange={(value) => setViewingTab(value as StageTab)}
        class="w-full"
        orientation="vertical"
      >
        {/* 左侧 tab 列表 */}
        <TabsList class="w-30">
          <For each={tabs()}>
            {(tab) => {
              const status = () => (tab !== "root" ? stage_map()[tab as StageName]?.status : null);
              return (
                <TabsTrigger value={tab} class="w-full justify-start">
                  <ContextMenu>
                    <ContextMenuTrigger class="w-full justify-start flex items-center gap-1.5">
                      <span class="flex-1 truncate">{tab}</span>
                      <Show when={status()}>
                        <StageStatusBadge
                          status={status()!}
                          progress={stage_map()[tab as StageName]?.progress}
                        />
                      </Show>
                    </ContextMenuTrigger>
                    <ContextMenuContent>
                      <Show when={tab !== "root"}>
                        <ContextMenuItem onSelect={() => handleResumeFrom(tab)}>
                          从这一阶段继续运行
                        </ContextMenuItem>
                      </Show>
                      <ContextMenuItem> 重新运行此阶段(开发中)</ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </TabsTrigger>
              );
            }}
          </For>
        </TabsList>

        {/* 右侧内容 */}
        <For each={tabs()}>
          {(tab) => (
            <TabsContent value={tab} class="overflow-auto p-0">
              <Show when={resumeFrom()}>
                <div class="flex items-center gap-1.5 px-3 py-1.5 border-b text-sm bg-muted/30 shrink-0">
                  <Play
                    class={`size-3 text-green-500 hover:text-green-400 cursor-pointer shrink-0 ${resume_task.isPending ? "pointer-events-none opacity-40" : ""}`}
                    onClick={() => {
                      if (resume_task.isPending) return;
                      handleConfirmResume();
                    }}
                  />
                  <span class="text-muted-foreground">继续阶段:</span>
                  <span class="font-medium text-foreground">{resumeFrom()}</span>
                </div>
              </Show>
              <Show when={viewingTab() === tab}>
                <FileTree relativeDir={tab === "root" ? taskDir : `${taskDir}/${tab}`} />
              </Show>
            </TabsContent>
          )}
        </For>
      </Tabs>
    </div>
  );
};
