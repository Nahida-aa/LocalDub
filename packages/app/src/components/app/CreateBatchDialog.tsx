import { createSignal, For, Show } from "solid-js";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openModal, closeModal } from "@repo/ui-solid/custom/modal/renderer";
import { Button } from "@repo/ui-solid/base/button";
import {
  FolderOpen,
  Play,
  Loader2,
  CheckCircle2,
  XCircle,
  AlertCircle,
  FileVideo,
} from "lucide-solid";
import { client } from "#/integrations/fnrpc/client.ts";

const videoFilters = [
  {
    name: "视频",
    extensions: ["mp4", "mkv", "mov", "avi", "webm", "flv", "ts", "m4v", "wmv"],
  },
];

// ============================================================
// 组件
// ============================================================
interface Props {
  onDone?: () => void;
}

type BatchFileInfo = { path: string; name: string };
type Status = "idle" | "picked" | "processing" | "done" | "error";

export function CreateBatchDialogContent(props: Props) {
  const queryClient = useQueryClient();
  const [status, setStatus] = createSignal<Status>("idle");
  const [files, setFiles] = createSignal<BatchFileInfo[]>([]);
  const [currentIdx, setCurrentIdx] = createSignal(0);
  const [taskResults, setTaskResults] = createSignal<
    { file: BatchFileInfo; success: boolean; error?: string }[]
  >([]);
  const [globalError, setGlobalError] = createSignal("");

  const startTask = useMutation(() => client.start_task.mutationOptions());

  const handlePickFiles = async () => {
    setGlobalError("");
    try {
      const selected = await openDialog({ multiple: true, filters: videoFilters });
      const list: BatchFileInfo[] = (Array.isArray(selected) ? selected : [selected])
        .filter((p): p is string => typeof p === "string")
        .map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path }));
      if (list.length === 0) return;
      setFiles(list);
      setStatus("picked");
    } catch (e: any) {
      setGlobalError(e?.message ?? String(e));
      setStatus("error");
    }
  };

  const handleStartBatch = async () => {
    const list = files();
    if (list.length === 0) return;

    setStatus("processing");
    setCurrentIdx(0);
    setTaskResults([]);

    const results: { file: BatchFileInfo; success: boolean; error?: string }[] = [];

    for (let i = 0; i < list.length; i++) {
      setCurrentIdx(i);
      try {
        await startTask.mutateAsync(list[i].path);
        results.push({ file: list[i], success: true });
      } catch (e: any) {
        results.push({
          file: list[i],
          success: false,
          error: e?.message ?? String(e),
        });
      }
      setTaskResults([...results]);
    }

    setStatus("done");
    queryClient.invalidateQueries({
      queryKey: client.get_group_list.queryKey(null),
    });
    props.onDone?.();
  };

  const handleClose = () => closeModal();

  const completed = () => taskResults().filter((r) => r.success).length;
  const failed = () => taskResults().filter((r) => !r.success).length;

  return (
    <div class="flex flex-col gap-4 p-2 min-w-[480px] max-w-[580px]">
      {/* 标题 */}
      <div class="flex items-center gap-2 text-lg font-semibold">
        <FolderOpen class="w-5 h-5" />
        <span>批量创建任务</span>
      </div>

      {/* 错误提示 */}
      <Show when={globalError()}>
        <div class="flex items-center gap-2 rounded-md bg-destructive/10 border border-destructive/30 p-3 text-sm text-destructive">
          <AlertCircle class="w-4 h-4 shrink-0" />
          <span>{globalError()}</span>
        </div>
      </Show>

      {/* idle: 选择文件按钮 */}
      <Show when={status() === "idle"}>
        <div class="flex flex-col items-center gap-3 py-8 border-2 border-dashed rounded-lg border-muted-foreground/30">
          <FileVideo class="w-10 h-10 text-muted-foreground" />
          <p class="text-sm text-muted-foreground text-center px-4">
            选择一个或多个视频文件，将为每个视频创建独立的配音任务
          </p>
          <Button onClick={handlePickFiles} class="gap-2">
            <FolderOpen class="w-4 h-4" />
            选择视频文件
          </Button>
        </div>
      </Show>

      {/* picked: 选择成功 → 显示文件列表 */}
      <Show when={status() === "picked"}>
        <div class="flex flex-col gap-4">
          <div class="text-xs text-muted-foreground mb-1.5">
            已选择 <span class="font-medium text-foreground">{files().length}</span> 个视频文件
          </div>
          <div class="max-h-[240px] overflow-y-auto border rounded-md">
            <For each={files()}>
              {(file) => (
                <div class="flex items-center gap-2 px-3 py-1.5 text-sm border-b border-border last:border-b-0">
                  <FileVideo class="w-3.5 h-3.5 text-muted-foreground shrink-0" />
                  <span class="truncate">{file.name}</span>
                </div>
              )}
            </For>
          </div>

          <div class="flex gap-2 justify-end">
            <Button variant="outline" onClick={handlePickFiles}>
              重新选择
            </Button>
            <Button onClick={handleStartBatch} class="gap-2">
              <Play class="w-4 h-4" />
              开始批量处理
            </Button>
          </div>
        </div>
      </Show>

      {/* processing */}
      <Show when={status() === "processing"}>
        <div class="flex flex-col gap-3">
          <div class="flex items-center gap-2">
            <Loader2 class="w-4 h-4 animate-spin" />
            <span class="text-sm">
              正在处理 {currentIdx() + 1} / {files().length}
            </span>
          </div>
          <div class="w-full h-1.5 bg-secondary rounded-full overflow-hidden">
            <div
              class="h-full bg-primary rounded-full transition-all duration-300"
              style={{
                width: `${((currentIdx() + 1) / files().length) * 100}%`,
              }}
            />
          </div>
          <Show when={files()[currentIdx()]}>
            <div class="flex items-center gap-2 text-sm text-muted-foreground">
              <FileVideo class="w-3.5 h-3.5" />
              <span class="truncate">{files()[currentIdx()].name}</span>
            </div>
          </Show>
          <Show when={taskResults().length > 0}>
            <div class="max-h-[160px] overflow-y-auto border rounded-md">
              <For each={taskResults()}>
                {(r) => (
                  <div class="flex items-center gap-2 px-3 py-1 text-xs border-b border-border last:border-b-0">
                    {r.success ? (
                      <CheckCircle2 class="w-3 h-3 text-green-500 shrink-0" />
                    ) : (
                      <XCircle class="w-3 h-3 text-destructive shrink-0" />
                    )}
                    <span class="truncate">{r.file.name}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </Show>

      {/* done */}
      <Show when={status() === "done"}>
        <div class="flex flex-col items-center gap-3 py-4">
          <CheckCircle2 class="w-10 h-10 text-green-500" />
          <p class="text-sm">
            批量任务完成：成功 {completed()} 个，失败 {failed()} 个
          </p>
          <Show when={failed() > 0}>
            <div class="w-full max-h-[160px] overflow-y-auto border rounded-md">
              <For each={taskResults().filter((r) => !r.success)}>
                {(r) => (
                  <div class="flex items-center gap-2 px-3 py-1 text-xs border-b border-border last:border-b-0">
                    <XCircle class="w-3 h-3 text-destructive shrink-0" />
                    <span class="truncate">{r.file.name}</span>
                    <Show when={r.error}>
                      <span class="text-muted-foreground truncate max-w-[200px]">- {r.error}</span>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>
          <Button variant="outline" onClick={handleClose}>
            关闭
          </Button>
        </div>
      </Show>

      {/* error state */}
      <Show when={status() === "error" && !globalError()}>
        <div class="flex flex-col items-center gap-3 py-4">
          <XCircle class="w-10 h-10 text-destructive" />
          <p class="text-sm">操作失败，请重试</p>
          <Button variant="outline" onClick={handlePickFiles}>
            重试
          </Button>
        </div>
      </Show>
    </div>
  );
}

export function openCreateBatchDialog(onDone?: () => void) {
  openModal(() => <CreateBatchDialogContent onDone={onDone} />, {
    size: "lg",
    title: "创建批量任务",
  });
}
