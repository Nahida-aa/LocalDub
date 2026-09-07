import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve, isAbsolute } from "node:path";
import envPaths from "env-paths";

const __dirname = import.meta.dir;

function hasWorkspaces(packageJsonPath: string): boolean {
  if (!existsSync(packageJsonPath)) return false;
  try {
    const pkg = JSON.parse(readFileSync(packageJsonPath, "utf8"));
    return Boolean(pkg.workspaces);
  } catch {
    return false;
  }
}

function findRepoRoot(startDir: string): string {
  const envRoot = process.env.REPO_ROOT?.trim();
  if (envRoot) return resolve(envRoot);
  let currentDir = startDir;
  for (;;) {
    if (hasWorkspaces(resolve(currentDir, "package.json"))) return currentDir;
    const parentDir = dirname(currentDir);
    if (parentDir === currentDir) break;
    currentDir = parentDir;
  }
  return resolve(startDir, "../../..");
}

export const REPO_ROOT = findRepoRoot(__dirname);

export const repo_root = () => REPO_ROOT;

export const base_dir = () => {
  const d = process.env.LOCALDUB_BASE_DIR?.trim();
  if (d) return resolve(d);
  if (process.env.NODE_ENV === "production") return envPaths("aa.localdub", { suffix: "" }).data;

  return REPO_ROOT;
};

export const config_dir = () => {
  if (process.env.NODE_ENV === "production") return envPaths("aa.localdub", { suffix: "" }).config;

  return REPO_ROOT;
};

export const resolve_path = (val: string) => {
  if (isAbsolute(val)) return val;
  return resolve(repo_root(), val);
};
