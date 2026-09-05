import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";
import { createClient, fetchTransport, tauriTransport } from "@fnrpc/client";
import { __procedureMeta, type Procedures } from "@repo/sdk/fnrpc/bindings";
import { isTauri } from "@tauri-apps/api/core";
import { findServer } from "@repo/core/servers/discovery";

// 非 Tauri (浏览器) 场景: 用 mDNS 发现主服务器地址, fallback 到本机默认端口。
// 由于 fetchTransport 的 url 是同步固定值, 而 mDNS 是异步的, 这里先同步 fallback
// 到本机默认, 再在后台异步发现真实地址并通过重新挂载的 transport 生效。
const DEFAULT_SERVER_URL = "http://127.0.0.1:19110/fnrpc";

async function discoverServerUrl(): Promise<string> {
  const info = await findServer("main");
  return `http://${info.host}:${info.port}/fnrpc`;
}

// 首次模块加载即触发异步发现 (best-effort), 供上层在准备就绪后替换 transport。
export const serverUrlPromise: Promise<string> = (async () => {
  const url = await discoverServerUrl();
  console.debug("[fnrpc] discovered server:", url);
  return url;
})();

const transport = (() => {
  try {
    if (isTauri()) {
      return tauriTransport(() => import("@tauri-apps/api/core"));
    }
  } catch {
    // ignore
  }
  // 非 Tauri: 先同步用本机默认; 跨机器发现由 serverUrlPromise 提供, 上层可按需重建。
  return fetchTransport({ url: DEFAULT_SERVER_URL });
})();

console.debug("Using transport");
export const fnrpc = createClient<Procedures>(transport, __procedureMeta);
console.debug("Created fnrpc");

export const client = createTanstackQueryUtils(fnrpc);
