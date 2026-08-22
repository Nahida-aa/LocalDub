import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { makeFetch } from "./http.ts";
import { parseNodes } from "./nodes.ts";
import {
  resolveSingBoxBin,
  startInstance,
  stopInstances,
  type SingBoxInstance,
} from "./singbox.ts";
import { testConnectivity } from "./connectivity.ts";
import { GradioClient } from "./gradio.ts";
import { loadSegments } from "./task.ts";
import { orchestrate, type SiteSpec } from "./orchestrator.ts";
import { buildReport, saveReport } from "./report.ts";

interface CliArgs {
  nodeFile: string;
  splitJson: string;
  vocalsDir: string;
  sites: SiteSpec[];
  maxConcurrent: number;
  maxSegments: number;
  requestTimeoutSec: number;
  retries: number;
  dryRun: boolean;
  workDir: string;
  maxNodes: number;
}

function parseArgs(argv: string[]): CliArgs {
  const get = (key: string): string | undefined => {
    const i = argv.indexOf(key);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const has = (key: string) => argv.includes(key);

  let nodeFile = get("--nodes") ?? process.env.TTS_NODES;
  let splitJson = get("--split") ?? process.env.TTS_SPLIT;
  let vocalsDir = get("--vocals") ?? process.env.TTS_VOCALS;

  // 支持 --paths <json>：从配置文件读路径（避免命令行中文编码问题）
  const pathsFile = get("--paths");
  const tmpPaths = join(
    import.meta.dir,
    "..",
    "..",
    "..",
    "..",
    "packages",
    "tmp",
    "tts-concurrency",
    "paths.json",
  );
  const pkgPaths = join(import.meta.dir, "..", "paths.json");
  const pathsPath =
    pathsFile ?? (existsSync(tmpPaths) ? tmpPaths : existsSync(pkgPaths) ? pkgPaths : undefined);
  if (pathsPath && existsSync(pathsPath)) {
    try {
      const p = JSON.parse(readFileSync(pathsPath, "utf8")) as {
        nodes?: string;
        split?: string;
        vocals?: string;
      };
      if (p.nodes && !get("--nodes")) nodeFile = p.nodes;
      if (p.split && !get("--split")) splitJson = p.split;
      if (p.vocals && !get("--vocals")) vocalsDir = p.vocals;
      console.log(`[paths] 从 ${pathsPath} 读取路径配置`);
    } catch (e) {
      console.warn(`[paths] 读取 ${pathsPath} 失败: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // 未显式指定时，自动发现 workfolder 下的 split_audio 目录
  if (!splitJson || !vocalsDir) {
    const workRoot = process.env.TTS_WORKFOLDER ?? "g:\\LocalDub\\workfolder";
    const g = new Bun.Glob("**/split_audio/split_audio.json");
    for (const p of g.scanSync({ cwd: workRoot, onlyFiles: true })) {
      splitJson ??= join(workRoot, p);
      vocalsDir ??= join(workRoot, dirname(p), "vocals");
      break;
    }
  }

  if (!nodeFile || !splitJson || !vocalsDir || !existsSync(nodeFile) || !existsSync(splitJson)) {
    console.error(
      "用法: bun run-concurrency-test.ts --nodes <订阅文件> [--split <split_audio.json> --vocals <vocals目录>] [选项]",
    );
    console.error(
      "未指定 --split/--vocals 时，会自动搜索 g:\\LocalDub\\workfolder 下的 split_audio 目录",
    );
    console.error("Windows 中文路径也可用环境变量 TTS_NODES/TTS_SPLIT/TTS_VOCALS 传入");
    console.error("选项:");
    console.error("  --max-concurrent <N>   每站点最多并行任务数（默认 2）");
    console.error("  --max-segments <N>     每任务只跑前 N 段验证链路（默认 0=全部 25 段）");
    console.error("  --request-timeout <S>  单请求超时秒（默认 180）");
    console.error("  --retries <N>          失败重试次数（默认 1）");
    console.error("  --dry-run              只测连通性，不跑 TTS");
    console.error("  --site <name=url>      自定义站点（可多次指定，默认魔乐 + HF Space）");
    console.error("  --max-nodes <N>        最多使用的节点总数（默认不限）");
    console.error("  --work-dir <dir>       工作目录（默认 packages/tmp/tts-concurrency）");
    process.exit(2);
  }

  const sites: SiteSpec[] = [];
  const siteArgs = argv.filter((a) => a === "--site");
  let si = argv.indexOf("--site");
  while (si >= 0) {
    const pair = argv[si + 1];
    if (pair && pair.includes("=")) {
      const [name, url] = pair.split("=");
      sites.push({ name, base: url, maxConcurrent: 1 });
    }
    si = argv.indexOf("--site", si + 1);
  }
  if (sites.length === 0) {
    sites.push(
      { name: "魔乐官方站", base: "https://voxcpm.modelbest.cn", maxConcurrent: 1 },
      { name: "HF Space", base: "https://openbmb-voxcpm-demo.hf.space", maxConcurrent: 1 },
    );
  }

  const maxConcurrent = Number(get("--max-concurrent") ?? 2);
  sites.forEach((s) => (s.maxConcurrent = maxConcurrent));

  return {
    nodeFile: resolve(nodeFile),
    splitJson: resolve(splitJson),
    vocalsDir: resolve(vocalsDir),
    sites,
    maxConcurrent,
    maxSegments: Number(get("--max-segments") ?? 0),
    requestTimeoutSec: Number(get("--request-timeout") ?? 180),
    retries: Number(get("--retries") ?? 1),
    dryRun: has("--dry-run"),
    workDir: resolve(
      get("--work-dir") ??
        process.env.TTS_WORKDIR ??
        join(import.meta.dir, "..", "..", "..", "..", "packages", "tmp", "tts-concurrency"),
    ),
    maxNodes: Number(get("--max-nodes") ?? 0),
  };
}

function allocPort(start = 20000): () => number {
  // 简单递增端口分配，避免与常见端口冲突
  let next = start;
  return () => next++;
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  console.log("=== VoxCPM 并发压测 ===");
  console.log("目标站点:", args.sites.map((s) => `${s.name} (${s.base})`).join(", "));

  const singBoxDir = join(args.workDir, "sing-box");
  const audioDir = join(args.workDir, "audio");
  const resultDir = join(args.workDir, "results");
  mkdirSync(singBoxDir, { recursive: true });
  mkdirSync(audioDir, { recursive: true });
  mkdirSync(resultDir, { recursive: true });

  // 1. 解析节点
  const nodes = parseNodes(args.nodeFile);
  console.log(`\n[1] 解析节点: 共 ${nodes.length} 个有效节点 (vless/hysteria2)`);
  if (args.maxNodes > 0) {
    nodes.length = Math.min(nodes.length, args.maxNodes);
    console.log(`    受 --max-nodes 限制，使用前 ${nodes.length} 个`);
  }

  // 2. 确保 sing-box core
  console.log("\n[2] 准备 sing-box core ...");
  const binPath = await resolveSingBoxBin(singBoxDir);

  // 3. 启动实例 + 连通性测试（逐站点分别测）
  console.log("\n[3] 启动 sing-box 实例并测试连通性 ...");
  const portAlloc = allocPort();
  const instances: SingBoxInstance[] = [];
  const usablePortsBySite = new Map<string, number[]>();
  let started = 0;

  for (const node of nodes) {
    if (started >= 8) {
      console.log("  达到最大实例数(8)，停止启动更多实例。");
      break;
    }
    const port = portAlloc();
    try {
      const inst = await startInstance(binPath, node, port, singBoxDir);
      instances.push(inst);
      started++;
    } catch (e) {
      console.log(`  节点 ${node.name} 启动失败: ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }
    // 对每个站点分别测连通性
    for (const site of args.sites) {
      const conn = await testConnectivity(node, port, site.base);
      if (conn.ok) {
        if (!usablePortsBySite.has(site.name)) usablePortsBySite.set(site.name, []);
        usablePortsBySite.get(site.name)!.push(port);
      } else {
        console.log(
          `  节点 ${node.name} → ${site.name} 连通失败: ${conn.error} (${conn.latencyMs}ms)`,
        );
      }
    }
  }
  console.log(
    `  实例启动 ${started} 个。各站点可用节点数: ${[...usablePortsBySite.entries()].map(([k, v]) => `${k}=${v.length}`).join(", ") || "无"}`,
  );

  if (args.dryRun) {
    console.log("\n[dry-run] 仅测连通性，跳过 TTS。");
    await stopInstances(instances);
    console.log("已清理所有 sing-box 实例。");
    return;
  }
  const totalUsable = [...usablePortsBySite.values()].reduce((a, v) => a + v.length, 0);
  if (totalUsable === 0) {
    console.error("没有可用节点，无法继续。");
    await stopInstances(instances);
    process.exit(1);
  }

  // 4. 加载任务段
  const segments = loadSegments(args.splitJson, args.vocalsDir, args.maxSegments);
  console.log(`\n[4] 任务段: ${segments.length} 段 (${args.splitJson})`);

  // 5. 调度
  console.log("\n[5] 开始并发任务 ...");
  const usedPorts = new Set<number>();
  const outcomes = await orchestrate({
    sites: args.sites,
    segments,
    taskOpts: {
      maxSegments: args.maxSegments,
      requestTimeoutSec: args.requestTimeoutSec,
      retries: args.retries,
      audioDir,
      resultDir,
    },
    makeClient: (site, port) => new GradioClient(site.base, makeProxyFetch(port)),
    nextPorts: (site, want) => {
      // 全局端口去重：每个 sing-box 端口（节点）只分配给一个站点的一个任务
      const ports = usablePortsBySite.get(site.name) ?? [];
      const taken: number[] = [];
      for (const p of ports) {
        if (usedPorts.has(p)) continue;
        usedPorts.add(p);
        taken.push(p);
        if (taken.length >= want) break;
      }
      return taken;
    },
  });

  // 6. 报告
  console.log("\n[6] 生成报告 ...");
  const report = buildReport(outcomes);
  saveReport(report, resultDir);

  // 7. 清理
  await stopInstances(instances);
  console.log("\n已清理所有 sing-box 实例。");
}

function makeProxyFetch(port: number): typeof fetch {
  return makeFetch(`http://127.0.0.1:${port}`) as typeof fetch;
}

main().catch((e) => {
  console.error("\n[error]", e);
  process.exit(1);
});
