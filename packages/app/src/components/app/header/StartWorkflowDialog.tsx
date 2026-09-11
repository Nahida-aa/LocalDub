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
import { parse } from "jsonc-parser";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import type { Input } from "@repo/sdk/fnrpc/bindings";

const videoFilters = [
  {
    name: "视频",
    extensions: ["mp4", "mkv", "mov", "avi", "webm", "flv", "ts", "m4v", "wmv"],
  },
];

export const StartWorkflowDialog = () => {
  return (
    <Tooltip gutter={4}>
      <TooltipTrigger
        class={buttonVariants({ variant: "icon", size: "xs" })}
        onClick={() => {
          openModal(StartWorkflowContent, {
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

/// 读仓库根 input.jsonc 作入队基准 Input, 仅覆盖 url/action, 其余 (stages 等) 保持用户全局配置。
/// 镜像 CLI 参数模式 (`cli workflow --action enqueue_start --url ...`: input.jsonc + 标量覆盖)。
async function loadBaseInput(): Promise<Input> {
  // 0.4.6 raw fnrpc 已由 transport 解开 `{json, meta}` 信封, 这里拿到的是原始文本。
  const raw = await fnrpc.read_app_file_text("input.jsonc");
  return parse(raw) as Input;
}

const StartWorkflowContent = () => {
  const [url, setUrl] = createSignal("");
  const [enqueue, setEnqueue] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const navigate = useNavigate();
  const qc = useQueryClient();

  const start_workflow = useMutation(() =>
    client.start_workflow.mutationOptions({
      onSuccess: (relDir) => {
        toastSuccess(`任务已创建: ${relDir}`);
        closeModal();
        qc.invalidateQueries({ queryKey: client.get_group_list.queryKey(null) });
        // relDir 形如 `workfolder/<group>/<workflow>`, 跳到任务页实时看 stage 徽章
        const parts = relDir.replace(/\\/g, "/").split("/").filter(Boolean);
        const [group, workflow] = parts.slice(-2);
        if (group && workflow) {
          navigate({ to: "/group/$id/$videoId", params: { id: group, videoId: workflow } });
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

  const submitEnqueue = async (u: string) => {
    setBusy(true);
    try {
      const base = await loadBaseInput();
      const input: Input = {
        ...base,
        command: "workflow",
        workflow: {
          ...(base.workflow ?? {}),
          action: "start",
          url: u,
        },
      };
      // u64 → string → BigInt (仅 meta 含 typeId=0 时), 模板字符串可直接展示
      const id = await fnrpc.enqueue_start(input);
      toastSuccess(`已加入队列 (id=${id})，队列 worker 将串行执行`);
      closeModal();
      qc.invalidateQueries({ queryKey: client.get_group_list.queryKey(null) });
    } catch (e) {
      toastError(e, "加入队列失败");
    } finally {
      setBusy(false);
    }
  };

  const submit = () => {
    const u = url().trim();
    if (!u) return;
    if (enqueue()) {
      submitEnqueue(u);
    } else {
      start_workflow.mutate(u);
    }
  };

  const pending = () => busy() || start_workflow.isPending;

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
      <label class="flex cursor-pointer items-center gap-2 text-sm text-muted-foreground select-none">
        <input
          type="checkbox"
          checked={enqueue()}
          onChange={(e) => setEnqueue(e.currentTarget.checked)}
          class="size-4 rounded border-input accent-primary"
        />
        <span>加入队列（串行执行，不立即开始）</span>
      </label>
      <Button onClick={submit} disabled={pending() || !url().trim()} class="w-full">
        <Show when={pending()} fallback={"开始"}>
          <Loader2 class="size-4 animate-spin" />
        </Show>
        <Show when={pending()}>运行中...</Show>
      </Button>
    </div>
  );
};
