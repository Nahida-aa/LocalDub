import { writeFileSync } from "node:fs";
import { join } from "node:path";
import type { SiteOutcome } from "./orchestrator.ts";

export interface TestReport {
  generatedAt: string;
  sites: SiteOutcome[];
}

function fmt(sec?: number): string {
  if (sec === undefined || Number.isNaN(sec)) return "-";
  return sec.toFixed(1) + "s";
}

function avg(list: number[]): number | undefined {
  if (list.length === 0) return undefined;
  return list.reduce((a, b) => a + b, 0) / list.length;
}

export function buildReport(outcomes: SiteOutcome[]): TestReport {
  return { generatedAt: new Date().toISOString(), sites: outcomes };
}

export function renderMarkdown(report: TestReport): string {
  const lines: string[] = [];
  lines.push("# VoxCPM 在线 TTS 并发压测报告");
  lines.push("");
  lines.push(`- 生成时间: ${report.generatedAt}`);
  lines.push("");

  // 1. 站点汇总表
  lines.push("## 站点并发能力汇总");
  lines.push("");
  lines.push(
    "| 站点 | 使用节点数 | 并行任务数 | 请求总数 | 成功段 | 失败段 | busy 起始(第几个请求) | 结论 |",
  );
  lines.push("| --- | --- | --- | --- | --- | --- | --- | --- |");
  for (const s of report.sites) {
    const anyRejected = s.tasks.some((t) => t.segments.some((g) => g.rejected));
    const verdict =
      s.aborted || anyRejected
        ? `busy: 第 ${s.firstRejectedAtRequest ?? "-"} 个请求被拒`
        : "完整跑完 25 段";
    lines.push(
      `| ${s.site.name} | ${s.startedTasks} | ${s.startedTasks} | ${s.totalRequests} | ${s.okSegments} | ${s.failSegments} | ${s.firstRejectedAtRequest ?? "-"} | ${verdict} |`,
    );
  }
  lines.push("");

  // 2. 任务明细
  lines.push("## 任务明细（每任务 = 1 节点，25 段串行）");
  lines.push("");
  lines.push("| 站点 | 任务 | 节点端口 | 段数 | 成功 | 失败 | 总耗时 | 每段平均耗时 | busy段 |");
  lines.push("| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
  for (const s of report.sites) {
    for (const t of s.tasks) {
      const lats = t.segments.map((g) => g.latencySec);
      lines.push(
        `| ${s.site.name} | ${t.taskId} | ${t.port} | ${t.segments.length} | ${t.okCount} | ${t.failCount} | ${fmt(t.totalSec)} | ${fmt(avg(lats))} | ${t.firstRejectedAt ?? "-"} |`,
      );
    }
  }
  lines.push("");

  // 3. 每请求耗时明细
  lines.push("## 每段请求耗时（秒）");
  lines.push("");
  for (const s of report.sites) {
    lines.push(`### ${s.site.name}`);
    lines.push("");
    lines.push("| 任务 | 段号 | 状态 | 耗时 | 说明 |");
    lines.push("| --- | --- | --- | --- | --- |");
    for (const t of s.tasks) {
      for (const g of t.segments) {
        lines.push(
          `| ${t.taskId} | ${g.segIdx} | ${g.ok ? "✅成功" : g.rejected ? "⛔被拒" : "❌失败"} | ${fmt(g.latencySec)} | ${g.error ?? g.audioPath ?? ""} |`,
        );
      }
    }
    lines.push("");
  }
  return lines.join("\n");
}

export function saveReport(report: TestReport, dir: string): string {
  const jsonPath = join(dir, "report.json");
  writeFileSync(jsonPath, JSON.stringify(report, null, 2));
  const md = renderMarkdown(report);
  const mdPath = join(dir, "results.md");
  writeFileSync(mdPath, md);
  console.log(`\n报告已保存: ${jsonPath}`);
  console.log(`Markdown 表格: ${mdPath}`);
  return mdPath;
}
