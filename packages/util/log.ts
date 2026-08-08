import { getLastSegment } from "./path";
import { nowISO } from "./time";
import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

// write log
export function write_log(taskDir: string, line: string) {
  const tid = getLastSegment(taskDir);
  console.log(line);
  if (!tid) return;
  const ts = nowISO();
  const logPath = join(taskDir, `${tid}.log`);
  appendFileSync(logPath, `[${ts}] ${line}\n`);
}
