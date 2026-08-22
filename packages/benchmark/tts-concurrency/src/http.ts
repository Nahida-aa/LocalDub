import { ProxyAgent, fetch as undiciFetch } from "undici";

/** 对外统一的 fetch 类型（兼容 DOM RequestInit/Response 用法） */
export type ProxyFetch = typeof fetch;

/** 创建一个可复用、带（可选）HTTP 代理的 fetch 函数 */
export function makeFetch(proxyUrl?: string): ProxyFetch {
  if (!proxyUrl) return undiciFetch as unknown as ProxyFetch;
  const agent = new ProxyAgent(proxyUrl);
  return ((url: RequestInfo | URL, init?: RequestInit) =>
    undiciFetch(
      url as never,
      {
        ...(init as object),
        dispatcher: agent,
      } as never,
    ) as unknown as Promise<Response>) as ProxyFetch;
}

/**
 * 解析 SSE 流。每个 data 行统一包装为 { _event, data }：
 * data 为原始 JSON 值（对象/数组/null/标量），避免数组/标量事件被丢弃或属性不可见。
 */
export async function readSse(res: Response): Promise<Array<Record<string, unknown>>> {
  if (!res.body) return [];
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let currentEvent = "";
  const events: Array<Record<string, unknown>> = [];
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      let idx: number;
      while ((idx = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, idx).trim();
        buf = buf.slice(idx + 1);
        if (line.startsWith("event:")) {
          currentEvent = line.slice(6).trim();
        } else if (line.startsWith("data:")) {
          const payload = line.slice(5).trim();
          let parsed: unknown = null;
          if (payload) {
            try {
              parsed = JSON.parse(payload);
            } catch {
              parsed = { raw: payload };
            }
          }
          events.push({ _event: currentEvent, data: parsed });
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
  return events;
}
