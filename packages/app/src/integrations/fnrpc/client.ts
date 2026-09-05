import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";
import { createClient, fetchTransport } from "@fnrpc/client";
import { __procedureMeta, type Procedures } from "@repo/sdk/fnrpc/bindings";
import { isTauri } from "@tauri-apps/api/core";

// fnrpc 基址:
// - Tauri 桌面: 主服务器由 app 启动时自动拉起 (本机 127.0.0.1:19110, 见 src-tauri lib.rs)
// - 主服务器直接 serve 的浏览器页面 (手机等): 同源
// - vite dev (1420) 的浏览器调试: 打本机 server (CORS permissive 已开)
function resolveServerUrl(): string {
  if (isTauri()) return "http://127.0.0.1:19110/fnrpc";
  if (typeof location !== "undefined" && location.port === "1420")
    return "http://127.0.0.1:19110/fnrpc";
  return `${location.origin}/fnrpc`;
}

const serverUrl = resolveServerUrl();

// 等 server ready (桌面启动时异步拉起, 有几秒窗口; 超时仍继续, 交给 retry/错误提示)
async function waitServerReady(base: string, timeoutMs = 30000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`${base}/health_check`, { signal: AbortSignal.timeout(2000) });
      if (r.ok) return;
    } catch {
      // 尚未就绪
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  console.warn("[fnrpc] server 未就绪 (超时), 继续加载但请求可能失败");
}

await waitServerReady(serverUrl);

export const fnrpc = createClient<Procedures>(fetchTransport({ url: serverUrl }), __procedureMeta);
export const client = createTanstackQueryUtils(fnrpc);
