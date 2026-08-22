import { REPO_ROOT } from "../root";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";

export function pythonBin(): string {
  // 允许通过环境变量覆盖 Python 解释器（如指向带 CUDA 的 conda 环境）
  const override = process.env.LOCALDUB_PYTHON;
  if (override) return override;
  const isWin = process.platform === "win32";
  return join(REPO_ROOT, ".venv", isWin ? "Scripts" : "bin", isWin ? "python.exe" : "python");
}

// scripts
export const VOXCPM_TORCH_GRADIO_MAIN = join(
  REPO_ROOT,
  "packages",
  "voxcpm_torch_server",
  "server.py",
);

/**
 * Find cmake.exe in common installation paths on Windows.
 */
export function findCmakePath(): string | null {
  const candidates = [
    join(process.env.ProgramFiles || "C:\\Program Files", "CMake", "bin", "cmake.exe"),
    join(
      process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)",
      "CMake",
      "bin",
      "cmake.exe",
    ),
    join(process.env.LOCALAPPDATA || "", "Microsoft", "WinGet", "Links", "cmake.exe"),
    join(
      process.env.USERPROFILE || "",
      "AppData",
      "Local",
      "Microsoft",
      "WinGet",
      "Links",
      "cmake.exe",
    ),
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  return null;
}

let _cmakePath: string | null = null;

/**
 * Returns the path to cmake binary, checking PATH first, then common install paths.
 */
export function cmakeBin(): string {
  if (_cmakePath) return _cmakePath;
  const fromPath = spawnSync("where", ["cmake"], { timeout: 5000, shell: true });
  if (fromPath.status === 0) {
    const lines = fromPath.stdout?.toString().trim().split(/\r?\n/);
    if (lines && lines.length > 0 && lines[0].length > 0) {
      _cmakePath = lines[0].trim();
      return _cmakePath;
    }
  }
  const found = findCmakePath();
  if (found) {
    _cmakePath = found;
    return _cmakePath;
  }
  return "cmake";
}
export function setCmakePath(path: string) {
  _cmakePath = path;
}
