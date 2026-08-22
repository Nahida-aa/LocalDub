import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { connect as netConnect } from "node:net";
import { join } from "node:path";
import type { ProxyNode } from "./nodes.ts";

export interface SingBoxInstance {
  node: ProxyNode;
  port: number;
  proc: ChildProcess;
  configPath: string;
}

export interface SingBoxManagerOptions {
  /** sing-box 可执行文件路径（优先用 SING_BOX_BIN，其次已下载的 core） */
  binPath: string;
  /** 工作目录：config 与日志输出地 */
  workDir: string;
}

/** 生成单个节点的 sing-box 配置（mixed inbound -> 节点 outbound） */
export function generateConfig(node: ProxyNode, port: number): Record<string, unknown> {
  const inboundTag = `mixed-${port}`;
  const outbound: Record<string, unknown> = {
    type: node.type,
    tag: "node",
    server: node.server,
    server_port: node.port,
  };

  if (node.type === "vless") {
    outbound.uuid = node.uuid;
    if (node.flow) outbound.flow = node.flow;
    outbound.tls = {
      enabled: true,
      server_name: node.servername ?? node.server,
      utls: { enabled: true, fingerprint: "firefox" },
      reality: {
        enabled: true,
        public_key: node.realityPublicKey,
        short_id: node.realityShortId,
      },
    };
  } else {
    // hysteria2
    outbound.password = node.password;
    outbound.tls = {
      enabled: true,
      server_name: node.sni ?? node.server,
      insecure: node.skipCertVerify ?? false,
    };
  }

  return {
    log: { level: "error", timestamp: true },
    inbounds: [
      {
        type: "mixed",
        tag: inboundTag,
        listen: "127.0.0.1",
        listen_port: port,
      },
    ],
    outbounds: [outbound, { type: "direct", tag: "direct" }],
    route: {
      rules: [{ inbound: [inboundTag], outbound: "node" }],
      final: "direct",
    },
  };
}

/**
 * 检查/下载 sing-box core。
 * 1) SING_BOX_BIN 环境变量指向的路径
 * 2) workDir/sing-box.exe（已下载）
 * 3) 从 GitHub releases 下载 windows-amd64 包
 */
export async function resolveSingBoxBin(workDir: string): Promise<string> {
  const envBin = process.env.SING_BOX_BIN;
  if (envBin && existsSync(envBin)) {
    return envBin;
  }

  const localBin = join(workDir, "sing-box.exe");
  if (existsSync(localBin)) {
    return localBin;
  }

  mkdirSync(workDir, { recursive: true });

  // 获取最新 release 版本
  let version = "";
  try {
    const res = await fetch("https://api.github.com/repos/SagerNet/sing-box/releases/latest", {
      headers: { "User-Agent": "tts-concurrency-benchmark" },
    });
    if (res.ok) {
      const j = (await res.json()) as { tag_name?: string };
      version = (j.tag_name ?? "").replace(/^v/, "");
    }
  } catch {
    // 忽略，用兜底版本
  }
  if (!version) version = "1.12.10";

  const zipName = `sing-box-${version}-windows-amd64`;
  const zipUrl = `https://github.com/SagerNet/sing-box/releases/download/v${version}/${zipName}.zip`;
  const zipPath = join(workDir, `${zipName}.zip`);

  console.log(`[sing-box] downloading core ${zipName} ...`);
  const resp = await fetch(zipUrl);
  if (!resp.ok) {
    throw new Error(
      `sing-box core download failed (${resp.status}). 请手动下载 ${zipUrl} 并解压出 sing-box.exe 放到 ${workDir}，或设置 SING_BOX_BIN 环境变量。`,
    );
  }
  const buf = Buffer.from(await resp.arrayBuffer());
  writeFileSync(zipPath, buf);

  // 解压：先解到临时目录再移动
  const extractDir = join(workDir, "extract");
  mkdirSync(extractDir, { recursive: true });

  // 用 PowerShell Expand-Archive 解压（Bun 无内置 zip）
  execFileSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `Expand-Archive -Path '${zipPath}' -DestinationPath '${extractDir}' -Force`,
    ],
    { stdio: "inherit" },
  );

  const found = walkForBin(extractDir);
  if (!found) {
    throw new Error(`无法在解压目录中找到 sing-box.exe: ${extractDir}`);
  }
  await import("node:fs/promises").then((fs) => fs.rename(found, localBin));

  // 清理
  const fs = await import("node:fs/promises");
  await fs.rm(extractDir, { recursive: true, force: true });
  await fs.rm(zipPath, { force: true });

  return localBin;
}

function walkForBin(dir: string): string | null {
  for (const name of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, name.name);
    if (name.isDirectory()) {
      const r = walkForBin(p);
      if (r) return r;
    } else if (name.name === "sing-box.exe") {
      return p;
    }
  }
  return null;
}

/**
 * 启动一个 sing-box 实例。
 * 等待 mixed 端口就绪后返回实例句柄。
 */
export async function startInstance(
  binPath: string,
  node: ProxyNode,
  port: number,
  workDir: string,
): Promise<SingBoxInstance> {
  const configPath = join(workDir, `config-${port}.json`);
  writeFileSync(configPath, JSON.stringify(generateConfig(node, port), null, 2));

  const proc = spawn(binPath, ["run", "-c", configPath, "--directory", workDir], {
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });

  const instance: SingBoxInstance = { node, port, proc, configPath };
  let stderrBuf = "";
  proc.stderr?.on("data", (d) => {
    stderrBuf += String(d);
  });

  // 等待端口就绪（最多 15s）
  const ready = await waitForPort(port, 15000);
  if (!ready) {
    proc.kill();
    throw new Error(
      `[sing-box] 实例启动失败 (port ${port}, node ${node.name}): ${stderrBuf.slice(-500)}`,
    );
  }
  return instance;
}

function waitForPort(port: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve) => {
    const probe = () => {
      if (Date.now() > deadline) return resolve(false);
      const sock = netConnect({ host: "127.0.0.1", port });
      sock.once("connect", () => {
        sock.destroy();
        resolve(true);
      });
      sock.once("error", () => {
        sock.destroy();
        setTimeout(probe, 300);
      });
    };
    probe();
  });
}

/** 停止所有 sing-box 实例 */
export async function stopInstances(instances: SingBoxInstance[]): Promise<void> {
  for (const inst of instances) {
    if (!inst.proc.killed) {
      try {
        inst.proc.kill();
      } catch {
        // 已退出
      }
    }
  }
  // 等 500ms 让进程退出，Windows 上再补 taskkill
  await new Promise((r) => setTimeout(r, 500));
}
