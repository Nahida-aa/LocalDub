import { useParams } from "@tanstack/solid-router";
import { For, Show, createSignal } from "solid-js";
import { FileTree } from "./FileTree";
import { Play } from "lucide-solid";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import {
  set_resumeFrom,
  setRunningStep,
  setViewingTab,
  StepTab,
  use_resumeFrom,
  useRunningStep,
  useViewingTab,
} from "./workflowControlPanelStore";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@repo/ui-solid/base/tabs";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@repo/ui-solid/base/context-menu";
import { StepStatusBadge } from "./StepStatusBadge";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { StepName, WorkflowCtx, WorkflowStep } from "@repo/sdk/index";

export const steps_to_map = (
  steps: (WorkflowStep | undefined)[],
): Record<StepName, WorkflowStep | undefined> => {
  return steps.reduce(
    (acc, step) => {
      if (step === undefined) return acc;
      acc[step.name as StepName] = step;
      return acc;
    },
    {} as Record<StepName, WorkflowStep | undefined>,
  );
};

export const WorkflowControlPanel = (p: {
  ctx: WorkflowCtx;
  // onResumeFrom: (stepName: string | null) => void;
}) => {
  const params = useParams({ from: "/group/$id/$videoId" });
  const videoDir = `workfolder/${params().id}/${p.ctx.workflow.id}`;
  const steps = () => p.ctx.steps ?? [];
  const step_map = () => steps_to_map(steps() ?? []);
  const tabs = () => ["root", ...steps().map((s) => s.name)] as StepTab[];
  const resumeFrom = use_resumeFrom();
  const runningStep = useRunningStep();
  const viewingTab = useViewingTab();
  const qc = useQueryClient();

  /**
   * 跳转到对应阶段前一个 tab 让用户确认\
   * 然后在内容界面点击运行按钮才会真的继续运行
   */
  const handleResumeFrom = (stepName?: StepTab | null) => {
    const allTabs = tabs();
    const idx = allTabs.indexOf(stepName ?? "root");
    if (idx > 0) {
      setViewingTab(allTabs[idx - 1]);
    }
    setRunningStep(stepName); // 高亮三角所在的当前 tab
    set_resumeFrom(stepName === "root" ? null : stepName);
  };

  const resume_workflow = useMutation(() =>
    client.continue_workflow.mutationOptions({
      onSuccess: () => {
        console.log("[continue] 继续运行 完成");
        // 运行结束后立即刷新 ctx 与文件树（watch 事件通常已覆盖，这里兜底）
        qc.invalidateQueries({
          queryKey: client.get_workflow_ctx.queryKey(videoDir),
        });
        qc.invalidateQueries({
          queryKey: client.list_app_directory.queryKey(videoDir),
        });
      },
      onError: (error) => {
        console.error("[continue] 继续运行 失败:", error);
      },
    }),
  );
  const handleConfirmResume = () => {
    const step = resumeFrom();
    if (!step) return;
    resume_workflow.mutate([videoDir, step]);
    set_resumeFrom(null);
    setViewingTab(runningStep());
  };

  return (
    <div class="w-100 min-w-40 border-r flex text-muted-foreground text-sm overflow-hidden">
      <Tabs
        value={viewingTab()}
        onChange={(value) => setViewingTab(value as StepTab)}
        class="w-full"
        orientation="vertical"
      >
        {/* 左侧 tab 列表 */}
        <TabsList class="w-30">
          <For each={tabs()}>
            {(tab) => {
              const status = () => (tab !== "root" ? step_map()[tab as StepName]?.status : null);
              return (
                <TabsTrigger value={tab} class="w-full justify-start">
                  <ContextMenu>
                    <ContextMenuTrigger class="w-full justify-start flex items-center gap-1.5">
                      <span class="flex-1 truncate">{tab}</span>
                      <Show when={status()}>
                        <StepStatusBadge
                          status={status()!}
                          progress={step_map()[tab as StepName]?.progress}
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
                    class={`size-3 text-green-500 hover:text-green-400 cursor-pointer shrink-0 ${resume_workflow.isPending ? "pointer-events-none opacity-40" : ""}`}
                    onClick={() => {
                      if (resume_workflow.isPending) return;
                      handleConfirmResume();
                    }}
                  />
                  <span class="text-muted-foreground">继续阶段:</span>
                  <span class="font-medium text-foreground">{resumeFrom()}</span>
                </div>
              </Show>
              <Show when={viewingTab() === tab}>
                <FileTree relativeDir={tab === "root" ? videoDir : `${videoDir}/${tab}`} />
              </Show>
            </TabsContent>
          )}
        </For>
      </Tabs>
    </div>
  );
};
