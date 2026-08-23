import { createStore, useSelector } from "@tanstack/solid-store";

export interface TabItem {
  path: string;
  label: string;
}

interface ContentPanelStore {
  activePath: string | null;
  tabs: TabItem[];
}
// You can instantiate the store outside of Solid components too!
export const contentPanelStore = createStore<ContentPanelStore>({
  activePath: null,
  tabs: [],
});

/// 媒体文件(视频/音频)刷新版本号。文件树事件命中某媒体路径时自增，对应
/// VideoPanel/AudioPanel 把版本拼到 `?v=` 上强制浏览器重新拉取，避开流式写入
/// 中途的脏缓存。注意：二进制走 axum ServeDir，不进 TanStack Query，因此不能用
/// invalidateQueries，必须靠 src 变化触发重拉。详见 .agents/skills 的 SKILL.md。
interface MediaVersionStore {
  /// path(相对 workfolder) -> 版本号
  versions: Record<string, number>;
}
export const mediaVersionStore = createStore<MediaVersionStore>({ versions: {} });

export const bumpMediaVersion = (relativePath: string) => {
  mediaVersionStore.setState((s) => ({
    versions: { ...s.versions, [relativePath]: (s.versions[relativePath] ?? 0) + 1 },
  }));
};

export const useMediaVersion = (relativePath: string) =>
  useSelector(mediaVersionStore, (s) => s.versions[relativePath] ?? 0);

export const useActivePath = () => useSelector(contentPanelStore, (state) => state.activePath);

export const setActivePath = (path: string | null) => {
  contentPanelStore.setState((state) => ({ ...state, activePath: path }));
};

export const useTabs = () => useSelector(contentPanelStore, (state) => state.tabs);
export const setTabs = (tabs: TabItem[]) => {
  contentPanelStore.setState((state) => ({ ...state, tabs }));
};

export const addTab = (tab: TabItem) => {
  contentPanelStore.setState((state) => ({ ...state, tabs: [...state.tabs, tab] }));
};
export const closeTab = (path: string) => {
  contentPanelStore.setState((state) => {
    const filtered = state.tabs.filter((t) => t.path !== path);
    // 如果关掉的是当前活跃 tab，切换到最后一个或 null
    if (path === state.activePath) {
      setActivePath(filtered.length > 0 ? filtered[filtered.length - 1].path : null);
    }
    return { ...state, tabs: filtered };
  });
};
