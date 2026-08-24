import { spawn, spawnSync } from "node:child_process";
import { existsSync, statSync, readFileSync, readdirSync, copyFileSync } from "node:fs";
import { join } from "node:path";
import {
  DEMUCS_MODEL_DIR,
  DEMUCS_GGML_FILE,
  WHISPER_MODEL_DIR,
  VOXCPM_MODEL_DIR,
} from "@repo/config/path/models";
import type { CheckResult } from "../../../env/types";
import { REPO_ROOT } from "@repo/config/root";
import { pythonBin } from "@repo/config/path/bin";
import { env } from "node:process";
import { checkOcrCppBin, ensureOcrCppBin } from "@repo/env/items/ocr_cpp_bin";

function tryExec(
  cmd: string,
  args: string[],
  cwd?: string,
): { ok: boolean; stdout: string; stderr: string } {
  try {
    const r = spawnSync(cmd, args, { timeout: 10_000, encoding: "utf-8", cwd } as any);
    return {
      ok: r.status === 0,
      stdout: ((r.stdout as string) || "").trim(),
      stderr: ((r.stderr as string) || "").trim(),
    };
  } catch {
    return { ok: false, stdout: "", stderr: "" };
  }
}

function isStale(binPath: string, watchPaths: string[]): boolean {
  if (!existsSync(binPath)) return false;
  const binTime = Math.floor(statSync(binPath).mtimeMs / 1000);
  for (const p of watchPaths) {
    const absPath = join(REPO_ROOT, p);
    if (!existsSync(absPath)) continue;
    const r = tryExec("git", ["log", "-1", "--format=%ct", "--", p], REPO_ROOT);
    if (!r.ok || !r.stdout.trim()) continue;
    if (parseInt(r.stdout.trim(), 10) > binTime) return true;
  }
  return false;
}

function getLatestSource(watchPaths: string[]): number {
  let latest = 0;
  for (const p of watchPaths) {
    const absPath = join(REPO_ROOT, p);
    if (!existsSync(absPath)) continue;
    const r = tryExec("git", ["log", "-1", "--format=%ct", "--", p], REPO_ROOT);
    if (r.ok && r.stdout.trim()) {
      const ts = parseInt(r.stdout.trim(), 10);
      if (ts > latest) latest = ts;
    }
  }
  return latest;
}

function fileSize(path: string): number | null {
  try {
    const s = statSync(path);
    return s.size;
  } catch {
    return null;
  }
}

function fmtSize(bytes: number): string {
  if (bytes > 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes > 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  if (bytes > 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

async function checkModel(path: string, key: string, minMB: number): Promise<CheckResult> {
  const size = fileSize(path);
  if (size === null) return { key, status: "fail", data: {}, required: false };
  const mb = size / 1e6;
  if (mb < minMB) return { key, status: "warn", data: { size: fmtSize(size) }, required: false };
  return { key, status: "pass", data: { size: fmtSize(size) }, required: false };
}

export async function checkBun(): Promise<CheckResult> {
  const r = tryExec("bun", ["--version"]);
  if (!r.ok) return { key: "bun", status: "fail", data: {}, required: true };
  return { key: "bun", status: "pass", data: { version: r.stdout }, required: true };
}

export async function checkPython(): Promise<CheckResult> {
  const pyBin = pythonBin();
  if (!existsSync(pyBin)) return { key: "python", status: "fail", data: {}, required: true };
  const r = tryExec(pyBin, ["--version"]);
  const ver = r.stdout.match(/(\d+\.\d+\.\d+)/)?.[0] || r.stdout;
  if (!r.ok) return { key: "python", status: "fail", data: {}, required: true };
  return { key: "python", status: "pass", data: { version: ver, path: pyBin }, required: true };
}

export async function checkUv(): Promise<CheckResult> {
  const r = tryExec("uv", ["--version"]);
  if (!r.ok) return { key: "uv", status: "fail", data: {}, required: true };
  const py = tryExec("uv", ["python", "find"]);
  return {
    key: "uv",
    status: "pass",
    data: { version: r.stdout.split(" ")[1] || r.stdout, pythonPath: py.ok ? py.stdout : "" },
    required: true,
  };
}

export async function checkFfmpeg(): Promise<CheckResult> {
  const bin = process.env.FFMPEG_PATH || "ffmpeg";
  const r = tryExec(bin, ["-version"]);
  if (!r.ok) return { key: "ffmpeg", status: "fail", data: {}, required: true };
  const ver = r.stdout.match(/ffmpeg version (\S+)/)?.[1] || "";
  const hasX264 = r.stdout.includes("libx264");
  const hasMp3 = r.stdout.includes("libmp3lame");
  const codecs =
    [hasX264 && "libx264", hasMp3 && "libmp3lame"].filter(Boolean).join(", ") || "none";
  return { key: "ffmpeg", status: "pass", data: { version: ver, codecs }, required: true };
}

export async function checkCargo(): Promise<CheckResult> {
  const r = tryExec("cargo", ["--version"]);
  if (!r.ok) return { key: "cargo", status: "fail", data: {}, required: false };
  const ver = r.stdout.match(/\d+\.\d+\.\d+/)?.[0] || r.stdout;
  return { key: "cargo", status: "pass", data: { version: ver }, required: false };
}

export async function checkVcpkg(): Promise<CheckResult> {
  if (process.platform !== "win32")
    return { key: "vcpkg", status: "skip", data: {}, required: false };
  const gitDir = join(REPO_ROOT, "submodule", "vcpkg", ".git");
  if (!existsSync(gitDir))
    return { key: "vcpkg", status: "fail", data: { kind: "submodule" }, required: false };
  const vcpkgExe = join(
    REPO_ROOT,
    "submodule",
    "vcpkg",
    process.platform === "win32" ? "vcpkg.exe" : "vcpkg",
  );
  const r = tryExec(vcpkgExe, ["--version"]);
  if (!r.ok) return { key: "vcpkg", status: "fail", data: { kind: "bootstrap" }, required: false };
  return { key: "vcpkg", status: "pass", data: {}, required: false };
}

export async function checkOpenai(): Promise<CheckResult> {
  const baseUrl = env.OPENAI_BASE_URL;
  const apiKey = env.OPENAI_API_KEY;
  if (!baseUrl || !apiKey) {
    return { key: "openai", status: "fail", data: { issues: "不存在" }, required: false };
  }
  const isLocal = baseUrl.includes("localhost") || baseUrl.includes("127.0.0.1");

  try {
    const res = await fetch(`${baseUrl}/models`, {
      headers: isLocal ? {} : { Authorization: `Bearer ${apiKey}` },
      signal: AbortSignal.timeout(5000),
    });

    if (res.ok) {
      const json = await res.json();
      const models = json.data?.length ?? "unknown";
      return {
        key: "openai",
        status: "pass",
        data: { baseUrl, models: `${models} models` },
        required: false,
      };
    }

    return {
      key: "openai",
      status: "warn",
      data: { issues: `HTTP ${res.status}: ${res.statusText}` },
      required: false,
    };
  } catch (err) {
    return {
      key: "openai",
      status: "fail",
      data: { issues: (err as Error).message },
      required: false,
    };
  }
}

export async function checkVulkan(): Promise<CheckResult> {
  const r = tryExec("vulkaninfo", ["--summary"]);
  if (!r.ok) return { key: "vulkan", status: "fail", data: {}, required: false };
  const line = r.stdout
    .split("\n")
    .find((l: string) => l.includes("GPU") || l.includes("deviceName"));
  const gpu = line?.split(":").pop()?.trim() || "";
  return { key: "vulkan", status: "pass", data: { gpu }, required: false };
}

export async function checkRocm(): Promise<CheckResult> {
  const r = tryExec("rocm-smi", []);
  if (!r.ok) return { key: "rocm", status: "fail", data: {}, required: false };
  return { key: "rocm", status: "pass", data: {}, required: false };
}

export async function checkCuda(): Promise<CheckResult> {
  const r = tryExec("nvidia-smi", []);
  if (!r.ok) return { key: "cuda", status: "fail", data: {}, required: false };
  const ver = r.stdout.match(/CUDA Version:\s+(\S+)/)?.[1] || "";
  return { key: "cuda", status: "pass", data: { version: ver }, required: false };
}

function checkSubmodule(path: string, key: string): CheckResult {
  return {
    key,
    status: existsSync(join(path, ".git")) ? "pass" : "fail",
    data: {},
    required: false,
  };
}

export async function checkSubmoduleWhisperCpp(): Promise<CheckResult> {
  return checkSubmodule(join(REPO_ROOT, "submodule", "whisper.cpp"), "submodule_whisper_cpp");
}

export async function checkSubmoduleDemucsCpp(): Promise<CheckResult> {
  return checkSubmodule(join(REPO_ROOT, "submodule", "demucs.cpp"), "submodule_demucs_cpp");
}

export async function checkSubmoduleDemucsRs(): Promise<CheckResult> {
  return checkSubmodule(join(REPO_ROOT, "submodule", "demucs-rs"), "submodule_demucs_rs");
}

export async function checkSubmoduleVoxcpmRs(): Promise<CheckResult> {
  return checkSubmodule(join(REPO_ROOT, "submodule", "voxcpm-rs"), "submodule_voxcpm_rs");
}

export async function checkWhisperBin(): Promise<CheckResult> {
  const ext = process.platform === "win32" ? ".exe" : "";
  const path = join(REPO_ROOT, "submodule", "whisper.cpp", "build", "bin", `whisper-vulkan${ext}`);
  if (!existsSync(path)) return { key: "whisper_bin", status: "fail", data: {}, required: false };
  const stale = isStale(path, ["submodule/whisper.cpp/"]);
  return { key: "whisper_bin", status: stale ? "warn" : "pass", data: { path }, required: false };
}

export async function checkDemucsGgmlBin(): Promise<CheckResult> {
  const ext = process.platform === "win32" ? ".exe" : "";
  const path = join(REPO_ROOT, "submodule", "demucs.cpp", "build", `demucs_mt.cpp.main${ext}`);
  if (!existsSync(path))
    return { key: "demucs_ggml_bin", status: "fail", data: {}, required: false };
  const stale = isStale(path, ["submodule/demucs.cpp/cli-apps/"]);
  return {
    key: "demucs_ggml_bin",
    status: stale ? "warn" : "pass",
    data: { path },
    required: false,
  };
}

export async function checkVoxcpmBurnBin(): Promise<CheckResult> {
  const dir = join(REPO_ROOT, "target", "release");
  if (!existsSync(dir))
    return { key: "voxcpm_burn_bin", status: "fail", data: {}, required: false };
  const files = readdirSync(dir).filter(
    (f: string) => f.startsWith("voxcpm-burn-") && !f.endsWith(".d"),
  );

  const expected = ["voxcpm-burn-wgpu", "voxcpm-burn-cpu", "voxcpm-burn-vulkan", "voxcpm-burn-tch"];
  const existing = new Set(files);
  const missingBins = expected.filter((e) => !existing.has(e));

  if (files.length === 0)
    return {
      key: "voxcpm_burn_bin",
      status: "fail",
      data: { missing_bins: missingBins.join(", ") },
      required: false,
    };

  const latestSource = getLatestSource(["packages/voxcpm-burn/", "submodule/voxcpm-rs/"]);
  const staleBins: string[] = [];
  const freshBins: string[] = [];

  for (const f of files) {
    const binPath = join(dir, f);
    if (latestSource > 0 && existsSync(binPath)) {
      const binTime = Math.floor(statSync(binPath).mtimeMs / 1000);
      if (binTime < latestSource) staleBins.push(f);
      else freshBins.push(f);
    }
  }

  return {
    key: "voxcpm_burn_bin",
    status: staleBins.length > 0 || missingBins.length > 0 ? "warn" : "pass",
    data: {
      stale_bins: staleBins.join(", "),
      fresh_bins: freshBins.join(", "),
      missing_bins: missingBins.join(", "),
      binaries: files.join(", "),
    },
    required: false,
  };
}

export async function checkDemucsBurnBin(): Promise<CheckResult> {
  const dir = join(REPO_ROOT, "target", "release");
  if (!existsSync(dir))
    return { key: "demucs_burn_bin", status: "fail", data: {}, required: false };
  const files = readdirSync(dir).filter(
    (f: string) => f.startsWith("demucs-burn-") && !f.endsWith(".d"),
  );

  const expected = [
    "demucs-burn-wgpu",
    "demucs-burn-cpu",
    "demucs-burn-tch",
    "demucs-burn-rocm",
    "demucs-burn-cuda",
  ];
  const existing = new Set(files);
  const missingBins = expected.filter((e) => !existing.has(e));

  if (files.length === 0)
    return {
      key: "demucs_burn_bin",
      status: "fail",
      data: { missing_bins: missingBins.join(", ") },
      required: false,
    };

  const latestSource = getLatestSource(["packages/demucs_burn/", "submodule/demucs-rs/"]);
  const staleBins: string[] = [];
  const freshBins: string[] = [];

  for (const f of files) {
    const binPath = join(dir, f);
    if (latestSource > 0 && existsSync(binPath)) {
      const binTime = Math.floor(statSync(binPath).mtimeMs / 1000);
      if (binTime < latestSource) staleBins.push(f);
      else freshBins.push(f);
    }
  }

  return {
    key: "demucs_burn_bin",
    status: staleBins.length > 0 || missingBins.length > 0 ? "warn" : "pass",
    data: {
      stale_bins: staleBins.join(", "),
      fresh_bins: freshBins.join(", "),
      missing_bins: missingBins.join(", "),
      binaries: files.join(", "),
    },
    required: false,
  };
}

export async function checkCmake(): Promise<CheckResult> {
  const r = tryExec("cmake", ["--version"]);
  if (!r.ok) return { key: "cmake", status: "fail", data: {}, required: false };
  const ver = r.stdout.match(/\d+\.\d+\.\d+/)?.[0] || r.stdout;
  return { key: "cmake", status: "pass", data: { version: ver }, required: false };
}

export async function checkGit(): Promise<CheckResult> {
  const r = tryExec("git", ["--version"]);
  if (!r.ok) return { key: "git", status: "fail", data: {}, required: false };
  const ver = r.stdout.match(/\d+\.\d+\.\d+/)?.[0] || r.stdout;
  return { key: "git", status: "pass", data: { version: ver }, required: false };
}

export async function checkWhisperGgml(): Promise<CheckResult> {
  return checkModel(join(WHISPER_MODEL_DIR, "ggml-large-v3-turbo.bin"), "whisper_ggml", 1500);
}

export async function checkWhisperVad(): Promise<CheckResult> {
  return checkModel(join(WHISPER_MODEL_DIR, "ggml-silero-v6.2.0.bin"), "whisper_vad", 0.5);
}

export async function checkWhisperSherpa(): Promise<CheckResult> {
  const dir = join(WHISPER_MODEL_DIR, "sherpa_onnx");
  const encoder = join(dir, "turbo-encoder.int8.onnx");
  const decoder = join(dir, "turbo-decoder.int8.onnx");
  const tokens = join(dir, "turbo-tokens.txt");
  const all = [encoder, decoder, tokens].every((f) => existsSync(f));
  if (!all) return { key: "whisper_sherpa", status: "fail", data: {}, required: false };
  return { key: "whisper_sherpa", status: "pass", data: {}, required: false };
}

export async function checkWhisperOnnx(): Promise<CheckResult> {
  return checkModel(join(WHISPER_MODEL_DIR, "encoder_model.onnx"), "whisper_onnx", 200);
}

export async function checkDemucsPth(): Promise<CheckResult> {
  return checkModel(join(DEMUCS_MODEL_DIR, "htdemucs_ft.safetensors"), "demucs_pth", 300);
}

export async function checkDemucsOnnx(): Promise<CheckResult> {
  const stems = ["drums", "bass", "other", "vocals"];
  const missing: string[] = [];
  for (const s of stems) {
    const p = join(DEMUCS_MODEL_DIR, `htdemucs_ft_${s}_fp16weights.onnx`);
    if (!existsSync(p)) missing.push(s);
  }
  const found = stems.length - missing.length;
  if (missing.length > 0) {
    const status: CheckResult["status"] = missing.length === stems.length ? "fail" : "warn";
    return {
      key: "demucs_onnx",
      status,
      data: { found, total: stems.length, missing: missing.join(", ") },
      required: false,
    };
  }
  return {
    key: "demucs_onnx",
    status: "pass",
    data: { found, total: stems.length },
    required: false,
  };
}

export async function checkDemucsGgml(): Promise<CheckResult> {
  return checkModel(DEMUCS_GGML_FILE, "demucs_ggml", 80);
}

export async function checkVoxcpm2Onnx(): Promise<CheckResult> {
  const files = [
    "voxcpm2_prefill.onnx",
    "voxcpm2_prefill.onnx.data",
    "voxcpm2_decode_step.onnx",
    "voxcpm2_decode_step.onnx.data",
    "audio_vae_decoder.onnx",
    "audio_vae_decoder.onnx.data",
    "audio_vae_encoder.onnx",
    "audio_vae_encoder.onnx.data",
  ];
  const missing: string[] = [];
  for (const f of files) {
    if (!existsSync(join(VOXCPM_MODEL_DIR, f))) missing.push(f);
  }
  const found = files.length - missing.length;
  if (missing.length > 0) {
    const status: CheckResult["status"] = missing.length === files.length ? "fail" : "warn";
    return {
      key: "voxcpm2_onnx",
      status,
      data: { found, total: files.length, missing: missing.join(", ") },
      required: false,
    };
  }
  return {
    key: "voxcpm2_onnx",
    status: "pass",
    data: { found, total: files.length },
    required: false,
  };
}

export async function checkVoxcpm2Pth(): Promise<CheckResult> {
  const model = join(VOXCPM_MODEL_DIR, "model.safetensors");
  const vae = join(VOXCPM_MODEL_DIR, "audiovae.pth");
  const modelOk = fileSize(model);
  const vaeOk = fileSize(vae);
  if (!modelOk || !vaeOk) {
    const missing = [!modelOk && "model.safetensors", !vaeOk && "audiovae.pth"]
      .filter(Boolean)
      .join(", ");
    return { key: "voxcpm2_pth", status: "fail", data: { missing }, required: false };
  }
  return {
    key: "voxcpm2_pth",
    status: "pass",
    data: { modelSize: fmtSize(modelOk), vaeSize: fmtSize(vaeOk) },
    required: false,
  };
}

export async function checkDotenv(): Promise<CheckResult> {
  const envPath = join(REPO_ROOT, ".env");
  if (!existsSync(envPath)) return { key: "dotenv", status: "fail", data: {}, required: false };
  const content = readFileSync(envPath, "utf-8");
  const hasDevice = content.includes("DEVICE=");
  const hasKey = content.includes("OPENAI_API_KEY=");
  const issues: string[] = [];
  if (!hasDevice) issues.push("DEVICE not set");
  if (!hasKey) issues.push("OPENAI_API_KEY not set");
  if (issues.length > 0)
    return { key: "dotenv", status: "warn", data: { issues: issues.join(", ") }, required: false };
  return { key: "dotenv", status: "pass", data: {}, required: false };
}

export const allChecks: Record<string, () => Promise<CheckResult>> = {
  bun: checkBun,
  python: checkPython,
  uv: checkUv,
  ffmpeg: checkFfmpeg,
  cargo: checkCargo,
  vcpkg: checkVcpkg,
  vulkan: checkVulkan,
  rocm: checkRocm,
  cuda: checkCuda,
  whisper_ggml: checkWhisperGgml,
  whisper_vad: checkWhisperVad,
  whisper_sherpa: checkWhisperSherpa,
  whisper_onnx: checkWhisperOnnx,
  demucs_pth: checkDemucsPth,
  demucs_onnx: checkDemucsOnnx,
  demucs_ggml: checkDemucsGgml,
  voxcpm2_onnx: checkVoxcpm2Onnx,
  voxcpm2_pth: checkVoxcpm2Pth,
  submodule_whisper_cpp: checkSubmoduleWhisperCpp,
  submodule_demucs_cpp: checkSubmoduleDemucsCpp,
  submodule_demucs_rs: checkSubmoduleDemucsRs,
  submodule_voxcpm_rs: checkSubmoduleVoxcpmRs,
  whisper_bin: checkWhisperBin,
  demucs_ggml_bin: checkDemucsGgmlBin,
  voxcpm_burn_bin: checkVoxcpmBurnBin,
  demucs_burn_bin: checkDemucsBurnBin,
  ocr_cpp_bin: checkOcrCppBin,
  cmake: checkCmake,
  git: checkGit,
  dotenv: checkDotenv,
  openai: checkOpenai,
};

export const ensureFns: Record<string, () => Promise<CheckResult>> = {
  dotenv: async () => {
    const src = join(REPO_ROOT, ".env.example");
    const dst = join(REPO_ROOT, ".env");
    if (existsSync(dst)) return { key: "dotenv", status: "pass", data: {}, required: false };
    if (!existsSync(src)) return { key: "dotenv", status: "fail", data: {}, required: false };
    copyFileSync(src, dst);
    return { key: "dotenv", status: "pass", data: {}, required: false };
  },
  openai: async () => {
    const rawUrl = env.OPENAI_BASE_URL || "";
    if (!rawUrl.includes("localhost") && !rawUrl.includes("127.0.0.1"))
      return {
        key: "openai",
        status: "skip",
        data: { issues: "not a local server", baseUrl: rawUrl },
        required: false,
      } as CheckResult;

    try {
      const res = await fetch(`${rawUrl}/models`, { signal: AbortSignal.timeout(2000) });
      if (res.ok)
        return {
          key: "openai",
          status: "pass",
          data: { baseUrl: rawUrl },
          required: false,
        } as CheckResult;
    } catch {}

    const ollamaBin = Bun.which("ollama");
    if (!ollamaBin)
      return {
        key: "openai",
        status: "fail",
        data: { issues: "ollama not found in PATH" },
        required: false,
      } as CheckResult;

    const proc = spawn(ollamaBin, ["serve"], { detached: true, stdio: "ignore" });
    proc.unref();

    for (let i = 0; i < 15; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      try {
        const res = await fetch(`${rawUrl}/models`, { signal: AbortSignal.timeout(1000) });
        if (res.ok)
          return {
            key: "openai",
            status: "pass",
            data: { baseUrl: rawUrl },
            required: false,
          } as CheckResult;
      } catch {}
    }

    return {
      key: "openai",
      status: "fail",
      data: { issues: "ollama serve did not respond after 15s" },
      required: false,
    } as CheckResult;
  },
  ocr_cpp_bin: ensureOcrCppBin,
};
