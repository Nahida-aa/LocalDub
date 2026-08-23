import { For, Show, createSignal } from "solid-js";
import { client, fnrpc } from "#/integrations/fnrpc/client.ts";
import { useQuery } from "@tanstack/solid-query";
import { addTab, setActivePath, useTabs } from "#/components/app/FileContent/store/ContentPanel.ts";
import { DirEntry } from "@repo/sdk/index";

function formatSize(bytes: number | bigint): string {
  const n = typeof bytes === "bigint" ? Number(bytes) : (bytes as number);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

interface FileTreeProps {
  relativeDir: string;
  // onOpenFile: (name: string, path: string) => void;
  depth?: number;
}

function FileTreeItem(props: {
  entry: DirEntry;
  fullPath: string;
  // onOpenFile: (name: string, path: string) => void;
  depth: number;
}) {
  const [expanded, setExpanded] = createSignal(false);
  const tabs = useTabs();
  if (props.entry.is_dir) {
    return (
      <div>
        <div
          class="flex items-center gap-1 px-2 py-0.5 cursor-pointer hover:bg-accent/40 truncate text-xs"
          style={{ "padding-left": `${props.depth * 16 + 8}px` }}
          onClick={() => setExpanded((e) => !e)}
        >
          <span class="text-[10px] w-3 shrink-0">{expanded() ? "▼" : "▶"}</span>
          <span class="text-muted-foreground">📁</span>
          <span class="truncate">{props.entry.name}/</span>
        </div>
        <Show when={expanded()}>
          <FileTree
            relativeDir={props.fullPath}
            // onOpenFile={props.onOpenFile}
            depth={props.depth + 1}
          />
        </Show>
      </div>
    );
  }

  return (
    <div
      class="flex items-center gap-1 px-2 py-0.5 cursor-pointer hover:bg-accent/40 truncate text-xs"
      style={{ "padding-left": `${props.depth * 16 + 8}px` }}
      onClick={() => {
        // 如果已打开则直接切换
        const existing = tabs().find((t) => t.path === props.fullPath);
        if (existing) {
          setActivePath(props.fullPath);
          return;
        }
        addTab({
          label: props.entry.name,
          path: props.fullPath,
        });
        setActivePath(props.fullPath);
        // props.onOpenFile(props.entry.name, props.fullPath)
      }}
    >
      <span class="w-3 shrink-0" />
      <span class="text-muted-foreground">📄</span>
      <span class="truncate">{props.entry.name}</span>
      <Show when={props.entry.size != null}>
        <span class="text-muted-foreground/50 ml-auto shrink-0">
          {formatSize(props.entry.size!)}
        </span>
      </Show>
    </div>
  );
}

export function FileTree(props: FileTreeProps) {
  const depth = props.depth ?? 0;
  const query = useQuery(() => client.list_app_directory.queryOptions(props.relativeDir));

  return (
    <Show
      when={query.isSuccess}
      fallback={
        <div class="px-2 py-1 text-xs text-muted-foreground">
          {query.isPending ? "Loading..." : query.isError ? "Failed to load" : ""}
        </div>
      }
    >
      <For each={query.data}>
        {(entry) => (
          <FileTreeItem
            entry={entry}
            fullPath={`${props.relativeDir}/${entry.name}`}
            // onOpenFile={props.onOpenFile}
            depth={depth}
          />
        )}
      </For>
    </Show>
  );
}
