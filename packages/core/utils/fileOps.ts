import {
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
  mkdirSync,
  existsSync,
  type WriteFileOptions,
} from "node:fs";
import { TaskCtx } from "@repo/core/context/context";
import { emitLog } from "@repo/core/stages/utils/utils";
import { log } from "@repo/util/log";

export function getLastSegment(path: string) {
  const last = path.replace(/\\/g, "/").split("/").filter(Boolean).pop();
  if (!last) throw new Error(`Invalid path: ${path}`);
  return last;
}

export type FileOp = "read" | "write" | "copy" | "rm" | "mkdir";

export function fileLog(op: FileOp, path: string, extra?: string) {
  log(`[File] ${op} ${path}${extra ? " " + extra : ""}`);
}

export function readJson<T = any>(path: string) {
  fileLog("read", path);
  return Bun.file(path).json() as Promise<T>;
}

export function writeFile(path: string, content: string | Buffer, ctx: TaskCtx) {
  writeFileSync(path, content);
  fileLog("write", path, `(${Buffer.byteLength(content)}B)`);
}

export function copyFile(src: string, dst: string, ctx: TaskCtx) {
  copyFileSync(src, dst);
  fileLog("copy", `${src} → ${dst}`);
}

export function removeFile(path: string, ctx: TaskCtx) {
  if (!existsSync(path)) return;
  rmSync(path);
  fileLog("rm", path);
}

export { existsSync, readFileSync, rmSync, mkdirSync, writeFileSync, copyFileSync };
