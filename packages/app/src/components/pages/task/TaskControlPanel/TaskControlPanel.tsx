import type { TaskCtx } from "#/integrations/fnrpc/bindings.ts";
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
import { StageName } from "@repo/core/cmd/tasks/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@repo/ui-solid/base/tabs";
import { stages_to_map } from "@repo/core/stages/utils/filtering";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { StageStatusBadge } from "./StageStatusBadge";
import { useMutation } from "@tanstack/solid-query";

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
    client.resume_task.mutationOptions({
      onSuccess: () => {
        console.log("[resume] 继续运行 完成");
      },
      onError: (error) => {
        console.error("[resume] 继续运行 失败:", error);
      },
    }),
  );
  const handleConfirmResume = () => {
    const stage = resumeFrom();
    console.log("[resume] taskDir:", taskDir, "stage:", stage); // ← 加这行
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
              const status = () => (tab !== "root" ? stage_map()[tab]?.status : null);
              return (
                <TabsTrigger value={tab} class="w-full justify-start">
                  <ContextMenu>
                    <ContextMenuTrigger class="w-full justify-start flex items-center gap-1.5">
                      <span class="flex-1 truncate">{tab}</span>
                      <Show when={status()}>
                        <StageStatusBadge status={status()!} />
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
                    class="size-3 text-green-500 hover:text-green-400 cursor-pointer shrink-0"
                    onClick={handleConfirmResume}
                  />
                  <span class="text-muted-foreground">继续阶段:</span>
                  <span class="font-medium text-foreground">{resumeFrom()}</span>
                </div>
              </Show>
              <FileTree relativeDir={tab === "root" ? taskDir : `${taskDir}/${tab}`} />
            </TabsContent>
          )}
        </For>
      </Tabs>
    </div>
  );
};
