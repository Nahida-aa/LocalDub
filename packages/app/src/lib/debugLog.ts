/// 迷你前端日志工具 (镜像 Rust tracing / RUST_LOG 的级别控制)。
///
/// 日志点用 `trace/debug/info/warn/error` 包裹, 是否输出取决于当前级别:
/// `VITE_LD_LOG_LEVEL` (env) → `localStorage['ld-log-level']` → 默认 `'warn'`。
/// 每次调用现读级别, 因此运行时改级别立即生效, 无需刷新。
export type LogLevel = "off" | "trace" | "debug" | "info" | "warn" | "error";

const STORAGE_KEY = "ld-log-level";

const ORDER: Record<LogLevel, number> = {
  off: -1,
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

function storedLevel(): LogLevel {
  if (typeof localStorage === "undefined") return "warn";
  const v = localStorage.getItem(STORAGE_KEY);
  if (v && v in ORDER) return v as LogLevel;
  return "warn";
}

export function getLogLevel(): LogLevel {
  const env = import.meta.env.VITE_LD_LOG_LEVEL as LogLevel | undefined;
  if (env && env in ORDER) return env;
  return storedLevel();
}

export function setLogLevel(level: LogLevel): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(STORAGE_KEY, level);
}

export function logEnabled(level: LogLevel): boolean {
  return ORDER[level] <= ORDER[getLogLevel()];
}

export function trace(...args: unknown[]): void {
  if (logEnabled("trace")) console.warn(...args);
}

export function debug(...args: unknown[]): void {
  if (logEnabled("debug")) console.debug(...args);
}

export function info(...args: unknown[]): void {
  if (logEnabled("info")) console.info(...args);
}

export function warn(...args: unknown[]): void {
  if (logEnabled("warn")) console.warn(...args);
}

export function error(...args: unknown[]): void {
  if (logEnabled("error")) console.error(...args);
}
