import { createSignal, For, Show } from "solid-js";
import { closeTab, contentPanelStore, setActivePath } from "./store/ContentPanel";
import { useSelector } from "@tanstack/solid-store";
import { scrollbarHidden } from "@repo/shared/styles/css";
import { cn } from "@repo/shared/lib/utils";

interface TabItem {
  path: string;
  label: string;
}

// interface Props {
//   // tabs: TabItem[];
//   // activePath: string | null;
//   onTabClick: (path: string) => void;
//   // onCloseTab: (path: string) => void;
// }

export function FileTab() {
  const activePath = useSelector(contentPanelStore, (state) => state.activePath);
  const tabs = useSelector(contentPanelStore, (state) => state.tabs);
  // 没有 tab 时不渲染任何内容

  return (
    <div
      class={cn(
        "flex items-center  border-b h-8 text-sm select-none overflow-x-auto",
        scrollbarHidden,
      )}
    >
      <For each={tabs()}>
        {(tab) => {
          const isActive = tab.path === activePath();
          return (
            <div
              class={`
                relative flex items-center gap-2 px-3 py-1.5 cursor-pointer
                border-r text-xs whitespace-nowrap
                ${isActive ? "bg-crust  " : " hover:bg-crust/50"}
              `}
              onClick={() => setActivePath(tab.path)}
            >
              <span>{tab.label}</span>
              <button
                class="ml-1 w-4 h-4 rounded-sm flex items-center justify-center hover:bg-accent/70"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.path);
                }}
                title="Close"
              >
                ✕
              </button>
              {/* 活跃 tab 底部高亮条 */}
              <Show when={isActive}>
                <div class="absolute bottom-0 left-0 right-0 h-px bg-primary" />
              </Show>
            </div>
          );
        }}
      </For>
    </div>
  );
}
