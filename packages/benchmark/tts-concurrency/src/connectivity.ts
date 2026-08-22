import { makeFetch } from "./http.ts";
import type { ProxyNode } from "./nodes.ts";

export interface ConnectivityResult {
  node: ProxyNode;
  port: number;
  ok: boolean;
  latencyMs: number;
  error?: string;
}

/**
 * 经本地代理端口测试到目标站点的连通性。
 * 使用 generate_204（google）与站点 /config 双探针：任一成功即视为连通。
 */
export async function testConnectivity(
  node: ProxyNode,
  port: number,
  siteBase: string,
  timeoutMs = 15000,
): Promise<ConnectivityResult> {
  const proxyFetch = makeFetch(`http://127.0.0.1:${port}`);
  const t0 = Date.now();
  const urls = ["https://www.gstatic.com/generate_204", `${siteBase}/config`];
  let lastErr: string | undefined;

  for (const url of urls) {
    try {
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), timeoutMs);
      const res = await proxyFetch(url, { signal: ctrl.signal, redirect: "follow" });
      clearTimeout(timer);
      if (res.status === 204 || res.status === 200) {
        return {
          node,
          port,
          ok: true,
          latencyMs: Date.now() - t0,
        };
      }
      lastErr = `HTTP ${res.status}`;
    } catch (e) {
      lastErr = e instanceof Error ? e.message : String(e);
    }
  }

  return {
    node,
    port,
    ok: false,
    latencyMs: Date.now() - t0,
    error: lastErr,
  };
}
