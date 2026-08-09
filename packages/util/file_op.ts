import {
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
  mkdirSync,
  existsSync,
  type WriteFileOptions,
} from "node:fs";

import { log } from "./log";

export type FileOp = "read" | "write" | "copy" | "rm" | "mkdir";

export function fileLog(op: FileOp, path: string, extra?: string) {
  log(`[File] ${op} ${path}${extra ? " " + extra : ""}`);
}

export function ensureDir(path: string) {
  if (existsSync(path)) return;
  mkdirSync(path, { recursive: true });
  fileLog("mkdir", path);
}

export function writeJson(path: string, data: any) {
  const raw = JSON.stringify(data, null, 2);
  writeFileSync(path, raw);
  const lines = raw.split("\n").length;
  fileLog("write", path, `(${raw.length}B, ${lines} lines)`);
}
