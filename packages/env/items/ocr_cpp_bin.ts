import { execSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";
import { REPO_ROOT } from "@repo/config/root";
import type { CheckResult } from "../types";

export function ocrCppBinPath(): string {
  const name = `subtitle_ocr_ort_cpp${process.platform === "win32" ? ".exe" : ""}`;
  const b = join(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp", "build");
  return [join(b, "Release", name), join(b, name)].find(existsSync) ?? join(b, name);
}

export function existsOcrCppBin(): boolean {
  return existsSync(ocrCppBinPath());
}

export async function checkOcrCppBin(): Promise<CheckResult> {
  const path = ocrCppBinPath();
  if (!existsSync(path)) return { key: "ocr_cpp_bin", status: "fail", data: {}, required: false };

  if (process.platform === "linux") {
    try {
      const out = execSync(`ldd "${path}"`, { encoding: "utf-8", timeout: 5000 });
      if (out.includes("not found"))
        return {
          key: "ocr_cpp_bin",
          status: "warn",
          data: { path, runtime: "missing_libs" },
          required: false,
        };
    } catch {}
  }

  try {
    const binTime = Math.floor(statSync(path).mtimeMs / 1000);
    const out = execSync("git log -1 --format=%ct -- packages/subtitle-ocr/ort-cpp/", {
      cwd: REPO_ROOT,
      encoding: "utf-8",
      timeout: 5000,
    });
    const srcTime = parseInt(out.trim(), 10);
    if (!isNaN(srcTime) && srcTime > binTime)
      return { key: "ocr_cpp_bin", status: "warn", data: { path }, required: false };
  } catch {}

  return { key: "ocr_cpp_bin", status: "pass", data: { path }, required: false };
}

export async function ensureOcrCppBin(): Promise<CheckResult> {
  const b = join(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp", "build");
  const s = join(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp");

  execSync(`rm -rf "${b}"`, { timeout: 5000 });

  const args = [`-S "${s}"`, `-B "${b}"`];
  if (process.platform === "win32") {
    const tc = join(REPO_ROOT, "submodule", "vcpkg", "scripts", "buildsystems", "vcpkg.cmake");
    args.push(`-DCMAKE_TOOLCHAIN_FILE="${tc}"`, "-DVCPKG_TARGET_TRIPLET=x64-windows");
  }
  execSync(`cmake ${args.join(" ")}`, { stdio: "inherit", timeout: 120_000 });
  execSync(`cmake --build "${b}" --config Release --parallel`, {
    stdio: "inherit",
    timeout: 300_000,
  });

  const ok = existsOcrCppBin();
  return {
    key: "ocr_cpp_bin",
    status: ok ? "pass" : "fail",
    data: { path: ocrCppBinPath() },
    required: false,
  };
}
