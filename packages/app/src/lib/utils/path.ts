import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { createSignal } from "solid-js";
import { fnrpc } from "#/integrations/fnrpc/client.ts";

// media root 绝对路径 (Tauri 下用): 启动时经 fnrpc get_workfolder 拉取。
// 拿到后 media URL 走 Tauri asset protocol 直读本地文件 —— 桌面 UI 不再依赖
// HTTP /media ServeDir (server 未跑 / 与独立 server 双开时 media 也一致)。
// 未就绪前回退 HTTP URL (server 在跑时内容等价)。
// asset scope 配置为 "**" (见 src-tauri/tauri.conf.json): workfolder 路径因机器而异。
const [workfolderRoot, setWorkfolderRoot] = createSignal<string | null>(null);

if (isTauri()) {
  fnrpc
    .get_workfolder()
    .then((root) => setWorkfolderRoot(root.replace(/\/+$/, "")))
    .catch((e) => console.warn("[media] get_workfolder 失败, media 继续走 HTTP:", e));
}

function joinMediaPath(root: string, path: string): string {
  return `${root}/${path.replace(/^\/+/, "")}`;
}

export const mediaUrl = (path: string) => {
  const root = workfolderRoot();
  if (root !== null) return convertFileSrc(joinMediaPath(root, path));
  return `http://localhost:19110/media/${path}`;
};
