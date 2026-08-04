import { createSignal, For, Show } from "solid-js";
import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { openModal, closeModal } from "@repo/ui-solid/custom/modal/renderer";
import { Button } from "@repo/ui-solid/base/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@repo/ui-solid/base/select";
import {
  FolderOpen,
  Play,
  Loader2,
  CheckCircle2,
  XCircle,
  AlertCircle,
  FileVideo,
  Languages,
} from "lucide-solid";
import { client } from "#/integrations/fnrpc/client.ts";
import type { BatchFolderResult, BatchFileInfo } from "#/integrations/fnrpc/bindings.ts";

// ============================================================
// 语言列表
// ============================================================
const LANG_NAMES: Record<string, string> = {
  en: "English",
  zh: "Chinese",
  vi: "Vietnamese",
  ja: "Japanese",
  ko: "Korean",
  fr: "French",
  de: "German",
  es: "Spanish",
  pt: "Portuguese",
  ru: "Russian",
  ar: "Arabic",
  hi: "Hindi",
  th: "Thai",
  id: "Indonesian",
  ms: "Malay",
  tl: "Tagalog",
  my: "Burmese",
  km: "Khmer",
  lo: "Lao",
  mn: "Mongolian",
  ne: "Nepali",
  ur: "Urdu",
  bn: "Bengali",
};

type LangOption = { value: string; label: string };
const AUTO_LANG: LangOption = { value: "__auto__", label: "自动检测 (Auto)" };
const LANG_OPTIONS: LangOption[] = [
  AUTO_LANG,
  ...Object.entries(LANG_NAMES).map(([code, name]) => ({
    value: code,
    label: `${name} (${code})`,
  })),
];

// ============================================================
// 组件
// ============================================================
interface Props {
  onDone?: () => void;
}

type Status = "idle" | "picked" | "lang_select" | "processing" | "done" | "error";

export function CreateBatchDialogContent(props: Props) {
  const queryClient = useQueryClient();
  const [status, setStatus] = createSignal<Status>("idle");
  const [folderResult, setFolderResult] = createSignal<BatchFolderResult | null>(null);
  const [currentIdx, setCurrentIdx] = createSignal(0);
  const [taskResults, setTaskResults] = createSignal<
    { file: BatchFileInfo; success: boolean; error?: string }[]
  >([]);
  const [globalError, setGlobalError] = createSignal("");

  // 语言选择
  const [sourceLang, setSourceLang] = createSignal<LangOption>(AUTO_LANG);
  const [targetLang, setTargetLang] = createSignal<LangOption>(AUTO_LANG);

  const startNewTask = useMutation(() =>
    client.start_new_task.mutationOptions()
  );

  const handlePickFolder = async () => {
    setGlobalError("");
    try {
      const result = await queryClient.fetchQuery(
        client.pick_batch_folder.queryOptions(null)
      );
      if (result) {
        setFolderResult(result);
        setStatus("lang_select");
      }
    } catch (e: any) {
      setGlobalError(e?.message ?? String(e));
      setStatus("error");
    }
  };

  const handleStartBatch = async () => {
    const f = folderResult();
    if (!f || f.files.length === 0) return;

    setStatus("processing");
    setCurrentIdx(0);
    setTaskResults([]);

    const files = f.files;
    const results: { file: BatchFileInfo; success: boolean; error?: string }[] = [];

    const srcLangParam =
      sourceLang().value === "__auto__" ? null : sourceLang().value;
    const tgtLangParam =
      targetLang().value === "__auto__" ? null : targetLang().value;

    for (let i = 0; i < files.length; i++) {
      setCurrentIdx(i);
      try {
        await startNewTask.mutateAsync([
          files[i].path,
          "dub",
          srcLangParam,
          tgtLangParam,
        ]);
        results.push({ file: files[i], success: true });
      } catch (e: any) {
        results.push({
          file: files[i],
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

      {/* idle: 选择文件夹按钮 */}
      <Show when={status() === "idle"}>
        <div class="flex flex-col items-center gap-3 py-8 border-2 border-dashed rounded-lg border-muted-foreground/30">
          <FileVideo class="w-10 h-10 text-muted-foreground" />
          <p class="text-sm text-muted-foreground text-center px-4">
            选择一个包含 MP4 视频文件的文件夹，将为每个视频创建独立的配音任务
          </p>
          <Button onClick={handlePickFolder} class="gap-2">
            <FolderOpen class="w-4 h-4" />
            选择文件夹
          </Button>
        </div>
      </Show>

      {/* lang_select: 选择文件夹成功 → 显示语言选择 + 文件列表 */}
      <Show when={status() === "lang_select"}>
        <Show when={folderResult()}>
          {(f) => (
            <div class="flex flex-col gap-4">
              {/* 文件夹信息 */}
              <div class="flex items-center gap-2 text-sm text-muted-foreground">
                <FolderOpen class="w-4 h-4" />
                <span class="font-medium text-foreground truncate">{f().folder_path}</span>
              </div>

              {/* 语言选择 */}
              <div class="flex gap-3">
                <div class="flex-1 flex flex-col gap-1.5">
                  <label class="text-xs font-medium text-muted-foreground">
                    源语言 (从)
                  </label>
                  <Select<LangOption>
                    value={sourceLang()}
                    optionValue="value"
                    optionTextValue="label"
                    onChange={(v) => {
                      if (v) setSourceLang(v);
                    }}
                    options={LANG_OPTIONS}
                    itemComponent={(itemProps) => (
                      <SelectItem item={itemProps.item}>
                        {itemProps.item.rawValue.label}
                      </SelectItem>
                    )}
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue<LangOption>>
                        {(state) => (
                          <div class="flex items-center gap-1.5">
                            <Languages class="w-3.5 h-3.5 text-muted-foreground" />
                            <span>{state.selectedOption().label}</span>
                          </div>
                        )}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent />
                  </Select>
                </div>

                <div class="flex-1 flex flex-col gap-1.5">
                  <label class="text-xs font-medium text-muted-foreground">
                    目标语言 (到)
                  </label>
                  <Select<LangOption>
                    value={targetLang()}
                    optionValue="value"
                    optionTextValue="label"
                    onChange={(v) => {
                      if (v) setTargetLang(v);
                    }}
                    options={LANG_OPTIONS}
                    itemComponent={(itemProps) => (
                      <SelectItem item={itemProps.item}>
                        {itemProps.item.rawValue.label}
                      </SelectItem>
                    )}
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue<LangOption>>
                        {(state) => (
                          <div class="flex items-center gap-1.5">
                            <Languages class="w-3.5 h-3.5 text-muted-foreground" />
                            <span>{state.selectedOption().label}</span>
                          </div>
                        )}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent />
                  </Select>
                </div>
              </div>

              {/* 文件列表 */}
              <div>
                <div class="text-xs text-muted-foreground mb-1.5">
                  找到 <span class="font-medium text-foreground">{f().files.length}</span> 个视频文件
                </div>
                <div class="max-h-[200px] overflow-y-auto border rounded-md">
                  <For each={f().files}>
                    {(file) => (
                      <div class="flex items-center gap-2 px-3 py-1.5 text-sm border-b border-border last:border-b-0">
                        <FileVideo class="w-3.5 h-3.5 text-muted-foreground shrink-0" />
                        <span class="truncate">{file.name}</span>
                      </div>
                    )}
                  </For>
                </div>
              </div>

              {/* 按钮 */}
              <div class="flex gap-2 justify-end">
                <Button variant="outline" onClick={handlePickFolder}>
                  重新选择
                </Button>
                <Button onClick={handleStartBatch} class="gap-2">
                  <Play class="w-4 h-4" />
                  开始批量处理
                </Button>
              </div>
            </div>
          )}
        </Show>
      </Show>

      {/* processing */}
      <Show when={status() === "processing"}>
        <Show when={folderResult()}>
          {(f) => {
            const total = () => f().files.length;
            const idx = () => currentIdx();
            const currentFile = () => f().files[idx()];
            return (
              <div class="flex flex-col gap-3">
                <div class="flex items-center gap-2">
                  <Loader2 class="w-4 h-4 animate-spin" />
                  <span class="text-sm">
                    正在处理 {idx() + 1} / {total()}
                  </span>
                </div>
                <div class="w-full h-1.5 bg-secondary rounded-full overflow-hidden">
                  <div
                    class="h-full bg-primary rounded-full transition-all duration-300"
                    style={{
                      width: `${((idx() + 1) / total()) * 100}%`,
                    }}
                  />
                </div>
                <Show when={currentFile()}>
                  <div class="flex items-center gap-2 text-sm text-muted-foreground">
                    <FileVideo class="w-3.5 h-3.5" />
                    <span class="truncate">{currentFile()?.name}</span>
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
            );
          }}
        </Show>
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
                      <span class="text-muted-foreground truncate max-w-[200px]">
                        - {r.error}
                      </span>
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
          <Button variant="outline" onClick={handlePickFolder}>
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
