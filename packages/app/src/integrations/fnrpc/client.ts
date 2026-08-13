import { createTanstackQueryUtils } from "@fnrpc/tanstack-query";
import { createClient, fetchTransport, tauriTransport } from "@fnrpc/client";
import { __procedureMeta, type Procedures } from "@repo/sdk/fnrpc/bindings";
import { isTauri } from "@tauri-apps/api/core";

const transport = (() => {
  try {
    if (isTauri()) {
      return tauriTransport(() => import("@tauri-apps/api/core"));
    }
  } catch {
    // ignore
  }
  return fetchTransport({ url: "http://localhost:19110/fnrpc" });
})();

console.debug("Using transport");
export const fnrpc = createClient<Procedures>(transport, __procedureMeta);
console.debug("Created fnrpc");

export const client = createTanstackQueryUtils(fnrpc);
