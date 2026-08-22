import type { GradioClient } from "./gradio.ts";
import { runTask, type Segment, type SegmentResult, type TaskResult } from "./task.ts";

export interface SiteSpec {
  name: string;
  base: string;
  /** 该站点最多并行任务数（每个任务占一个节点） */
  maxConcurrent: number;
}

export interface SiteOutcome {
  site: SiteSpec;
  /** 实际启动的任务数（=使用的节点数） */
  startedTasks: number;
  /** 该站点累计发出的请求数（含成功/失败/被拒） */
  totalRequests: number;
  /** 第几个请求开始被拒绝（从 1 计）；未拒绝则 undefined */
  firstRejectedAtRequest?: number;
  /** 成功完成的段数 */
  okSegments: number;
  failSegments: number;
  aborted: boolean;
  tasks: TaskResult[];
}

export interface OrchestrateOptions {
  sites: SiteSpec[];
  segments: Segment[];
  taskOpts: Parameters<typeof runTask>[6];
  /** (site, port) => GradioClient 工厂（client 已带代理） */
  makeClient: (site: SiteSpec, port: number) => GradioClient;
  /** 按顺序为站点提供可用节点端口池；返回某站点实际应使用的端口列表 */
  nextPorts: (site: SiteSpec, want: number) => number[];
  onSegment?: (site: SiteSpec, seg: SegmentResult) => void;
}

/** 站点级调度：每站最多 maxConcurrent 个任务并行，busy 时停止该站点 */
export async function orchestrate(opts: OrchestrateOptions): Promise<SiteOutcome[]> {
  const outcomes: SiteOutcome[] = [];
  for (const site of opts.sites) {
    const state: SiteOutcome = {
      site,
      startedTasks: 0,
      totalRequests: 0,
      okSegments: 0,
      failSegments: 0,
      aborted: false,
      tasks: [],
    };
    outcomes.push(state);

    const ports = opts.nextPorts(site, site.maxConcurrent);
    if (ports.length === 0) {
      console.log(`[${site.name}] 无可用的连通节点，跳过该站点。`);
      continue;
    }
    state.startedTasks = ports.length;

    const shouldStop = () => state.aborted;

    const taskPromises = ports.map(async (port) => {
      const client = opts.makeClient(site, port);
      // 动态探测签名（魔乐 10 参数 / HF 8 参数）
      await client.probe();

      const result = await runTask(
        client,
        `task-${site.name}-${port}`,
        site.name,
        `port-${port}`,
        port,
        opts.segments,
        {
          ...opts.taskOpts,
          shouldStop,
          onSegment: (seg) => {
            if (seg.rejected && state.firstRejectedAtRequest === undefined) {
              state.firstRejectedAtRequest = state.totalRequests + 1;
              state.aborted = true;
              console.log(
                `[${site.name}] ⚠ 站点开始拒绝请求（第 ${state.totalRequests + 1} 个请求，段 ${seg.segIdx}），停止该站点测试。`,
              );
            }
            state.totalRequests++;
            if (seg.ok) state.okSegments++;
            else state.failSegments++;
            opts.onSegment?.(site, seg);
          },
        },
      );
      return result;
    });

    const settled = await Promise.allSettled(taskPromises);
    settled.forEach((s) => {
      if (s.status === "fulfilled") state.tasks.push(s.value);
      else {
        console.error(`[${site.name}] 任务异常:`, s.reason);
        state.failSegments += opts.segments.length;
      }
    });
  }
  return outcomes;
}
