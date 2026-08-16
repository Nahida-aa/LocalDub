import { isTauri } from "@tauri-apps/api/core";
import { open as openDialog, type OpenDialogOptions } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "@tanstack/solid-router";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { FolderOpen, Plus, Loader2 } from "lucide-solid";
import { Show, createSignal } from "solid-js";
import { Button, buttonVariants } from "@repo/ui-solid/base/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@repo/ui-solid/base/dialog";
import { TextField, TextFieldInput, TextFieldLabel } from "@repo/ui-solid/base/text-field";
import { toastError, toastSuccess } from "@repo/ui-solid/custom/toast";
import { TooltipX } from "@repo/ui-solid/custom/tooltip";
import { client } from "#/integrations/fnrpc/client.ts";

const videoFilters = [
  {
    name: "视频",
    extensions: ["mp4", "mkv", "mov", "avi", "webm", "flv", "ts", "m4v", "wmv"],
  },
];

export const StartTaskDialog = () => {
  const [open, setOpen] = createSignal(false);
  const [url, setUrl] = createSignal("");
  const navigate = useNavigate();
  const qc = useQueryClient();

  const start_task = useMutation(() =>
    client.start_task.mutationOptions({
      onSuccess: (relDir) => {
        toastSuccess(`任务已创建: ${relDir}`);
        setOpen(false);
        setUrl("");
        qc.invalidateQueries({ queryKey: client.get_group_list.queryKey(null) });
        // relDir 形如 `workfolder/<group>/<task>`, 跳到任务页实时看 stage 徽章
        const parts = relDir.replace(/\\/g, "/").split("/").filter(Boolean);
        const [group, task] = parts.slice(-2);
        if (group && task) {
          navigate({ to: "/group/$id/$taskId", params: { id: group, taskId: task } });
        }
      },
      onError: (e) => toastError(e, "开始任务失败"),
    }),
  );

  const pickFile = async () => {
    try {
      const opts: OpenDialogOptions = { multiple: false, filters: videoFilters };
      const file = await openDialog(opts);
      if (typeof file === "string") setUrl(file);
    } catch (e) {
      toastError(e, "选择文件失败");
    }
  };

  const submit = () => {
    const u = url().trim();
    if (!u) return;
    start_task.mutate(u);
  };

  return (
    <Dialog open={open()} onOpenChange={setOpen}>
      <TooltipX content={`开始任务`} class={buttonVariants({ variant: "icon", size: "xs" })}>
        <Plus size={16} onClick={() => setOpen(true)} />
      </TooltipX>
      <DialogContent class="p-4 gap-3" size="md" showCloseButton>
        <DialogHeader>
          <DialogTitle>开始任务</DialogTitle>
          <DialogDescription>
            输入本地视频路径或远程 URL，导入后运行完整 pipeline。
          </DialogDescription>
        </DialogHeader>
        <TextField class="gap-1.5">
          <TextFieldLabel>视频地址</TextFieldLabel>
          <div class="flex items-center gap-1.5">
            <TextFieldInput
              class="min-w-0"
              placeholder="/path/to/video.mp4 或远程链接"
              value={url()}
              onInput={(e) => setUrl(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <Show when={isTauri()}>
              <Button variant="icon" size="icon-sm" onClick={pickFile} title="选择本地文件">
                <FolderOpen size={16} />
              </Button>
            </Show>
          </div>
        </TextField>
        <DialogFooter>
          <Button onClick={submit} disabled={start_task.isPending || !url().trim()}>
            <Show when={start_task.isPending} fallback={"开始"}>
              <Loader2 class="size-4 animate-spin" />
            </Show>
            <Show when={start_task.isPending}>运行中...</Show>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
