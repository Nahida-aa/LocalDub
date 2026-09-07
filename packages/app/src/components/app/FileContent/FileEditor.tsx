import { createSignal, onMount, onCleanup, createEffect, Match, Switch } from "solid-js";
import { getAutoSaveMode } from "../settings/editorPrefs.ts";
import { useTheme } from "@repo/ui-solid/theme";
import { fnrpc, client } from "#/integrations/fnrpc/client.ts";
import { loadMonacoTheme } from "../settings/loadTheme.ts";
import { createMutation, useMutation, useQuery } from "@tanstack/solid-query";
import { Loader2, LoaderCircleIcon } from "lucide-solid";
import { Show } from "solid-js/types/server/rendering.js";

const AUTO_SAVE_DELAY = 2000;

const EXT_LANG: Record<string, string> = {
  json: "json",
  jsonc: "json", // jsonc: monaco json tokenizer 高亮 (含注释); 诊断/校验放宽见下
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  rs: "rust",
  py: "python",
  toml: "plaintext",
  yaml: "yaml",
  yml: "yaml",
  md: "markdown",
  css: "css",
  html: "html",
  srt: "plaintext",
  vtt: "plaintext",
  txt: "plaintext",
  csv: "plaintext",
  xml: "xml",
  log: "plaintext",
  svelte: "html",
  vue: "html",
};

function detectLang(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase();
  return ext ? (EXT_LANG[ext] ?? "plaintext") : "plaintext";
}

function resolveRelative(filePath: string, ref: string): string {
  if (ref.startsWith("/")) return ref.slice(1);
  const dir = filePath.split("/").slice(0, -1).join("/");
  const normalized = ref.startsWith("./") ? ref.slice(2) : ref;
  return dir ? `${dir}/${normalized}` : normalized;
}

interface Props {
  path: string;
  label: string;
}

export function FileEditor(props: Props) {
  const { themeName } = useTheme();
  let containerRef: HTMLDivElement | undefined;
  // 长期持有的 editor instance（不随 tab 切换销毁）
  let editor: any = null;
  // 当前 model — 随 path 变化而创建新 model，旧的 dispose
  let currentModel: any = null;
  let autoSaveTimer: ReturnType<typeof setTimeout> | undefined;
  // lastSavedContent 跟随当前文件的保存状态
  const [dirty, setDirty] = createSignal(false);
  const [pathKey, setPathKey] = createSignal(props.path);

  // 每个文件独立维护 savedContent（用 Map 存储，而不是 single value）
  const savedContentMap = new Map<string, string>();

  const updateDirty = () => {
    if (!editor) return;
    const saved = savedContentMap.get(props.path) ?? "";
    setDirty(editor.getValue() !== saved);
  };

  const writeFile = useMutation(() => client.write_app_file_text.mutationOptions());
  /** 读取文件内容 */
  const fileQ = useQuery(() => client.read_app_file_text.queryOptions(props.path));
  // 初始内容（首次 mount 时用）
  let initialContent: string | undefined;

  /** 保存文件 */
  async function doSave(): Promise<void> {
    if (!editor || !currentModel) return;
    const content = editor.getValue();
    try {
      if (props.path.endsWith(".json")) JSON.parse(content);
      await writeFile.mutateAsync([props.path, content]);
      clearTimeout(autoSaveTimer);
      savedContentMap.set(props.path, content);
      setDirty(false);
    } catch {
      alert("Invalid JSON");
    }
  }

  /** 防抖自动保存 */
  function debouncedSave(content: string) {
    clearTimeout(autoSaveTimer);
    autoSaveTimer = setTimeout(async () => {
      try {
        if (props.path.endsWith(".json")) JSON.parse(content);
        await writeFile.mutateAsync([props.path, content]);
        savedContentMap.set(props.path, content);
        setDirty(false);
      } catch {
        /* silent */
      }
    }, AUTO_SAVE_DELAY);
  }

  let monacoInitialized = false;
  let monaco: typeof import("monaco-editor") | null = null;

  onMount(async () => {
    if (!containerRef || fileQ.isLoading) return;

    // 读取初始内容
    initialContent = fileQ.data ?? "";
    savedContentMap.set(props.path, initialContent);

    if (!monacoInitialized) {
      monacoInitialized = true;
      (self as any).MonacoEnvironment = {
        getWorker: async (mod: string, label: string) => {
          if (label === "json") {
            const jsonMod = await import("monaco-editor/esm/vs/language/json/json.worker?worker");
            return new jsonMod.default();
          }
          const baseMod = await import("monaco-editor/esm/vs/editor/editor.worker?worker");
          return new baseMod.default();
        },
      };
    }

    monaco = await import("monaco-editor");

    const lang = detectLang(props.path);
    const content = initialContent ?? "";

    // 加载主题
    const { themeByName } = await import("@repo/ui-solid/theme/defs");
    const def = themeByName(themeName());
    let monacoTheme = "vs-dark";
    if (def) {
      try {
        const themeFile = def.monacoTheme.replace(/[\\/:"*?<>|]/g, "_");
        await loadMonacoTheme(monaco, themeFile, def.value);
        monacoTheme = def.value;
      } catch {
        /* fallback */
      }
    }

    // JSON/JSONC schema 诊断: jsonc 允许注释与尾逗号; $schema 直接正则提取
    // (jsonc 内容 JSON.parse 会失败, 不做整文件解析)。
    if (lang === "json" && content) {
      try {
        const monacoJson = (monaco.languages.json as any)?.jsonDefaults;
        const diagOpts: Record<string, unknown> = {
          validate: true,
          allowComments: true, // jsonc: 注释
          allowTrailingCommas: true, // jsonc: 尾逗号
        };
        const schemaMatch = content.match(/"\$schema"\s*:\s*"([^"]+)"/);
        if (schemaMatch) {
          const schemaRef: string = schemaMatch[1];
          if (!schemaRef.startsWith("http")) {
            const schemaPath = schemaRef.startsWith("/")
              ? schemaRef.slice(1)
              : resolveRelative(props.path, schemaRef);
            try {
              const schemaContent = await fnrpc.read_app_file_text(schemaPath);
              const schemaObj = JSON.parse(schemaContent);
              diagOpts.schemas = [{ uri: `file://${schemaPath}`, schema: schemaObj }];
            } catch {
              /* schema not found */
            }
          }
        }
        monacoJson?.setDiagnosticsOptions(diagOpts);
      } catch {
        /* ignore */
      }
    }

    // 创建 model + editor（仅第一次）
    currentModel = monaco.editor.createModel(
      content,
      lang,
      monaco.Uri.parse(`file://${props.path}`),
    );
    editor = monaco.editor.create(containerRef, {
      model: currentModel,
      language: lang,
      theme: monacoTheme,
      automaticLayout: true,
      fontSize: 13,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      padding: { top: 8 },
      tabSize: 2,
      quickSuggestions: true,
      wordWrap: "on",
    });

    const autoSaveMode = getAutoSaveMode();
    editor.onDidChangeModelContent(() => {
      updateDirty();
      if (autoSaveMode === "afterDelay") {
        debouncedSave(editor.getValue());
      }
    });
  });

  // ✅ 核心：path 变化时只换 model，不重建 editor
  let prevPathRef = props.path;
  createEffect(() => {
    const newPath = props.path;
    if (newPath === prevPathRef || fileQ.isLoading) return;
    prevPathRef = newPath;
    setPathKey(newPath);

    const newLang = detectLang(newPath);
    if (!monaco) return;
    savedContentMap.set(newPath, fileQ.data!);

    // dispose 旧 model
    currentModel?.dispose();

    // 创建新 model 并绑定到已有 editor
    // console.log('FileEditor.createEffect.fetchContent.content:', content)
    currentModel = monaco.editor.createModel(
      fileQ.data!,
      newLang,
      monaco.Uri.parse(`file://${newPath}`),
    );
    editor.setModel(currentModel);
    setDirty(false);
  });

  onCleanup(() => {
    clearTimeout(autoSaveTimer);
    editor?.dispose();
    currentModel?.dispose();
  });

  return (
    <div class="flex flex-col h-full">
      <div class="flex items-center gap-2 px-3 py-1.5 text-sm border-b rounded-t-lg select-none">
        <svg
          class="w-3.5 h-3.5 shrink-0"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z" />
          <polyline points="14 2 14 8 20 8" />
        </svg>
        <span>{props.label}</span>
        <span
          class="text-sm cursor-pointer bg-primary rounded-full size-1.5 shrink-0"
          classList={{ "opacity-0": !dirty() }}
          onClick={doSave}
          title="Unsaved changes — click to save"
        ></span>
      </div>
      <Switch>
        <Match when={fileQ.isLoading}>
          <LoaderCircleIcon /> 加载中...
        </Match>
        <Match when={fileQ.isSuccess}>
          <div
            ref={containerRef}
            class="border-x border-b border-gray-700 overflow-hidden h-full"
          />
        </Match>
      </Switch>
    </div>
  );
}
