import { readFileSync } from "node:fs";
import { parse } from "yaml";

/** 规范化后的代理节点（仅 vless / hysteria2，已过滤占位说明节点） */
export interface ProxyNode {
  name: string;
  type: "vless" | "hysteria2";
  server: string;
  port: number;
  /** vless */
  uuid?: string;
  flow?: string;
  servername?: string;
  realityPublicKey?: string;
  realityShortId?: string;
  /** hysteria2 */
  password?: string;
  skipCertVerify?: boolean;
  sni?: string;
  /** 是否 IPv6 服务器地址（连通性测试注意） */
  isIPv6: boolean;
}

const PLACEHOLDER_PATTERN =
  /剩余流量|距离下次重置|套餐到期|每次使用前更新|有超过20多个|客户端设置|电报群|防失联页/;

function detectIPv6(server: string): boolean {
  // 含 ':' 且非域名结尾：视为 IPv6 字面地址
  return server.includes(":");
}

/**
 * 解析 mihomo 订阅 YAML，提取有效 vless/hysteria2 节点。
 * 跳过占位说明节点（名字带提示文案）与不支持的协议。
 */
export function parseNodes(filePath: string): ProxyNode[] {
  const raw = readFileSync(filePath, "utf8");
  const doc = parse(raw) as { proxies?: unknown[] };
  const proxies = Array.isArray(doc.proxies) ? doc.proxies : [];
  const nodes: ProxyNode[] = [];

  for (const p of proxies) {
    if (!p || typeof p !== "object") continue;
    const rec = p as Record<string, unknown>;
    const name = String(rec.name ?? "");
    const type = String(rec.type ?? "");
    const server = String(rec.server ?? "");
    const port = Number(rec.port ?? 0);

    if (!name || !server || !port) continue;
    if (PLACEHOLDER_PATTERN.test(name)) continue;
    if (type !== "vless" && type !== "hysteria2") continue;

    const node: ProxyNode = {
      name,
      type,
      server,
      port,
      isIPv6: detectIPv6(server),
    };

    if (type === "vless") {
      node.uuid = String(rec.uuid ?? "");
      node.flow = rec.flow ? String(rec.flow) : undefined;
      node.servername = rec.servername ? String(rec.servername) : undefined;
      const ro = rec["reality-opts"] as Record<string, unknown> | undefined;
      if (ro && typeof ro === "object") {
        node.realityPublicKey = ro["public-key"] ? String(ro["public-key"]) : undefined;
        node.realityShortId = ro["short-id"] ? String(ro["short-id"]) : undefined;
      }
    } else {
      node.password = String(rec.password ?? "");
      node.sni = rec.sni ? String(rec.sni) : undefined;
      node.skipCertVerify = rec["skip-cert-verify"] === true || rec["skip-cert-verify"] === "true";
    }

    nodes.push(node);
  }
  return nodes;
}
