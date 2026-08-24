import { isTauri } from "@tauri-apps/api/core";
import { open as openDialog, type OpenDialogOptions } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "@tanstack/solid-router";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { FolderOpen, Loader2, Plus } from "lucide-solid";
import { Show, createSignal } from "solid-js";
import { buttonVariants } from "@repo/ui-solid/base/button";
import { Button } from "@repo/ui-solid/base/button";
import { TextField, TextFieldInput } from "@repo/ui-solid/base/text-field";
import { Tooltip, TooltipContent, TooltipTrigger } from "@repo/ui-solid/base/tooltip";
import { toastError, toastSuccess } from "@repo/ui-solid/custom/toast";
import { closeModal, openModal } from "@repo/ui-solid/custom/modal/renderer";
import { client } from "#/integrations/fnrpc/client.ts";

const videoFilters = [
  {
    name: "视频",
    extensions: ["mp4", "mkv", "mov", "avi", "webm", "flv", "ts", "m4v", "wmv"],
  },
];

export const StartTaskDialog = () => {
  return (
    <Tooltip gutter={4}>
      <TooltipTrigger
        class={buttonVariants({ variant: "icon", size: "xs" })}
        onClick={() => {
          openModal(StartTaskContent, {
            title: "开始任务",
            description: "输入视频地址，或点击上方区域选择本地文件",
            size: "sm",
            showCloseButton: true,
          });
        }}
      >
        <Plus size={16} />
      </TooltipTrigger>
      <TooltipContent>开始任务</TooltipContent>
    </Tooltip>
  );
};

const StartTaskContent = () => {
  const [url, setUrl] = createSignal("");
  const navigate = useNavigate();
  const qc = useQueryClient();

  const start_task = useMutation(() =>
    client.start_task.mutationOptions({
      onSuccess: (relDir) => {
        toastSuccess(`任务已创建: ${relDir}`);
        closeModal();
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
    <div class="flex flex-col gap-3 pt-2">
      <Show when={isTauri()}>
        <button
          type="button"
          onClick={pickFile}
          class="flex h-24 flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-input text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
        >
          <FolderOpen size={20} />
          <span class="text-sm">点击选择本地文件</span>
        </button>
      </Show>
      <TextField>
        <TextFieldInput
          placeholder="/path/to/video.mp4 或远程链接"
          value={url()}
          onInput={(e) => setUrl(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
        />
      </TextField>
      <Button onClick={submit} disabled={start_task.isPending || !url().trim()} class="w-full">
        <Show when={start_task.isPending} fallback={"开始"}>
          <Loader2 class="size-4 animate-spin" />
        </Show>
        <Show when={start_task.isPending}>运行中...</Show>
      </Button>
    </div>
  );
};
