import { getLastSegment } from "./path";
import { nowISO } from "./time";
import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { AsyncLocalStorage } from "node:async_hooks";

// 全局日志上下文：运行开始时 set 一次 taskDir，之后调用 log() 免传路径。
// 落盘契约（不可改）：文件名为 `<taskDir 最后一段>.log`，位于 taskDir 下，
// 追加写（append-only）。行格式保留 `[ts] line\n` 开头；若设置了 currentStage，
// 则在时间戳后追加 `[stage] ` 前缀，即 `[ts] [stage] line\n`。
// Rust/Tauri 端独立复刻了该命名规则并做增量 tail（只读取文本、不解析内容），
// 因此新增前缀不会断掉前端日志流。
interface LogContext {
  taskDir: string;
  currentStage?: string;
}

const als = new AsyncLocalStorage<LogContext>();

/**
 * 运行开始时调用一次：把日志上下文注入当前 async 执行链。
 * 使用 enterWith，使本次设置对后续（同调用链的）await 串行执行的 stage 可见，
 * 之后调用 log() 自动落到 `<tid>.log`，无需再传 taskDir。
 *
 * 注意：enterWith 会影响当前执行上下文及其继承者；在 pipeline 串行执行模型下安全。
 * 若未来出现跨 async 资源（新建 Promise 链 head / worker）的日志调用，store 可能丢失，
 * 此时 log() 会降级为仅 stdout 打印（见 log()）。
 */
export function setLogContext(taskDir: string): void {
  als.enterWith({ taskDir });
}

/**
 * 进入某个 stage 时调用：更新当前阶段名（保留 taskDir）。
 * 日志行随后会自动带 `[stage] ` 前缀。pipeline-runner 在每个 stage 开始前调用。
 */
export function setCurrentStage(stage: string): void {
  const cur = als.getStore() ?? { taskDir: "" };
  als.enterWith({ ...cur, currentStage: stage });
}

/** 当前日志上下文（测试/降级用）。外部一般无需调用。 */
export function getLogContext(): LogContext | undefined {
  return als.getStore();
}

function resolveLogPath(taskDir: string): string | null {
  let tid: string;
  try {
    tid = getLastSegment(taskDir);
  } catch {
    return null;
  }
  if (!tid) return null;
  return join(taskDir, `${tid}.log`);
}

/**
 * 共享落盘逻辑：打印到 stdout，并追加到 `<tid>.log`。
 * taskDir 为空时仅打印不落盘（pipeline 外的工具调用降级路径）。
 * currentStage 非空时，在内容前加 `[stage] ` 前缀。
 */
function appendToLog(taskDir: string | undefined, line: string, stage?: string): void {
  const prefix = stage ? `[${stage}] ` : "";
  console.log(prefix + line);
  if (!taskDir) return;
  const logPath = resolveLogPath(taskDir);
  if (!logPath) return;
  mkdirSync(dirname(logPath), { recursive: true });
  appendFileSync(logPath, `[${nowISO()}] ${prefix}${line}\n`);
}

/**
 * 写一行日志：打印到 stdout，并（若有日志上下文）追加到 `<tid>.log`。
 * 无上下文时仅打印不落盘（pipeline 外的工具调用降级路径）。
 */
export function log(line: string): void {
  const ctx = als.getStore();
  appendToLog(ctx?.taskDir, line, ctx?.currentStage);
}

/**
 * 兼容旧签名：显式传入 taskDir（优先级最高，不依赖 ALS）。
 * 用于尚未迁移的调用点或 pipeline 之外的工具。
 */
export function write_log(taskDir: string, line: string): void {
  const stage = als.getStore()?.currentStage;
  appendToLog(taskDir, line, stage);
}
