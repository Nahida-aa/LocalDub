/**
 * Auto-start Torch server if not already running.
 * Used by pipeline stages (separate, asr_ocr) to ensure the Python server is available.
 */
import { spawn, spawnSync } from "node:child_process";
import { delimiter, dirname, join } from "node:path";
import { homedir } from "node:os";
import { pythonBin } from "@repo/config/path/bin";
import { demucs_torch_server } from "@repo/config/path/scripts";
import { REPO_ROOT } from "@repo/config/root";
import { findServer } from "./discovery";

async function healthCheck(baseUrl: string): Promise<boolean> {
  try {
    const res = await fetch(`${baseUrl}/status`, {
      signal: AbortSignal.timeout(2000),
    });
    return res.ok;
  } catch {
    return false;
  }
}

/**
 * Locate ffmpeg so whisper/pydub subprocess calls succeed inside the server process.
 * ensureTorchServer restructures PATH and strips conda env vars; we resolve ffmpeg
 * upfront and inject its directory.
 */
function findFfmpegDir(venvBase: string): string | null {
  // 1) FFMPEG_PATH env var (absolute path → extract dirname)
  const explicit = process.env.FFMPEG_PATH;
  if (explicit && explicit !== "ffmpeg") {
    return dirname(explicit);
  }

  // 2) where ffmpeg on the current (outer) PATH
  try {
    const r = spawnSync("where", ["ffmpeg"], { encoding: "utf-8", timeout: 3000 });
    if (r.status === 0 && r.stdout) {
      const first = r.stdout.split(/[\r\n]+/)[0]?.trim();
      if (first) return dirname(first);
    }
  } catch {
    /* ignore */
  }

  return null;
}

/**
 * 并发防抖单例：ensureTorchServer 被多个阶段并发调用时（如 scheduler 并发跑多个
 * separate/asr 任务），每个调用都会先 healthCheck —— 若此时服务尚未就绪，全部调用
 * 都会误判"没起来"→ 各自 spawn 一个 uvicorn → 多进程抢绑 19109 → WinError 10048。
 * 修复：首个调用者负责 spawn，其余 await 同一 promise。失败/超时后清空，允许重试。
 */
let torchServerPromise: Promise<string> | null = null;

/**
 * Ensure the Demucs Torch server (port 19109) is running.
 * Returns the base URL (http://127.0.0.1:PORT).
 * If already running, returns immediately. Otherwise starts it and waits up to 60s.
 */
export async function ensureTorchServer(): Promise<string> {
  const { port } = await findServer("demucs_torch_server");
  const baseUrl = `http://127.0.0.1:${port}`;

  if (await healthCheck(baseUrl)) {
    console.log(`[TorchServer] Already running at ${baseUrl}`);
    return baseUrl;
  }

  if (!torchServerPromise) {
    torchServerPromise = spawnTorchServer(port).finally(() => {
      torchServerPromise = null; // 成功或失败都清空：成功后下次走 healthCheck 快路径；失败后允许重试
    });
  }
  return torchServerPromise;
}

/** 实际 spawn 一个 uvicorn torch server 并轮询就绪（仅供 ensureTorchServer 调用） */
async function spawnTorchServer(port: number): Promise<string> {
  const baseUrl = `http://127.0.0.1:${port}`;
  console.log("[TorchServer] Spawning ML torch server...");
  const voxcpmSrc = join(REPO_ROOT, "submodule", "VoxCPM", "src");
  const venvBase = join(REPO_ROOT, ".venv");

  const env: Record<string, string> = {
    ...(process.env as Record<string, string>),
    TORCHAUDIO_USE_BACKEND: "soundfile",
    VIRTUAL_ENV: venvBase,
    WHISPER_DOWNLOAD_ROOT: join(homedir(), ".cache", "whisper"),
    // Reduce CUDA memory fragmentation on small (≤6 GiB) GPUs.
    // See https://docs.pytorch.org/docs/stable/notes/cuda.html#optimizing-memory-usage-with-pytorch-cuda-alloc-conf
    PYTORCH_CUDA_ALLOC_CONF: "expandable_segments:True",
  };

  // Find ffmpeg on the host so whisper/pydub subprocess calls don't get FileNotFoundError.
  const ffmpegDir = findFfmpegDir(venvBase);
  if (ffmpegDir) {
    console.log(`[TorchServer] Found ffmpeg in: ${ffmpegDir}`);
  }

  // Prepend torch/torchaudio lib dirs to PATH so Windows DLL loader finds them first.
  // Without this, stale Conda DLLs in the outer PATH shadow the venv ones → libtorchaudio.pyd loads wrong deps.
  const torchLib = join(venvBase, "Lib", "site-packages", "torch", "lib");
  const torchAudioLib = join(venvBase, "Lib", "site-packages", "torchaudio", "lib");
  const venvScripts = join(venvBase, "Scripts");
  const dllPath = [torchLib, torchAudioLib, venvScripts]
    .concat(ffmpegDir ? [ffmpegDir] : [])
    .concat((env.PATH || "").split(delimiter).filter(Boolean))
    .join(delimiter);
  env.PATH = dllPath;

  // Unset conda env vars to prevent interference
  delete env.CONDA_PREFIX;
  delete env.CONDA_DEFAULT_ENV;
  delete env.CONDA_PROMPT_MODIFIER;

  const existingPy = env.PYTHONPATH || "";
  env.PYTHONPATH = existingPy ? `${voxcpmSrc}${delimiter}${existingPy}` : voxcpmSrc;

  const proc = spawn(pythonBin(), [demucs_torch_server, "--http-port", String(port)], {
    env,
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  proc.stderr?.pipe(process.stderr);
  proc.unref();

  // Poll health endpoint until ready
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 200));
    if (await healthCheck(baseUrl)) {
      console.log(`[TorchServer] Server ready at ${baseUrl} (pid ${proc.pid})`);
      return baseUrl;
    }
  }

  throw new Error(`TorchServer startup timeout after 60000ms`);
}

/**
 * Gracefully shut down the Torch server to free GPU + RAM between batch tasks.
 * Returns the base URL if a server was found and shut down, null otherwise.
 */
export async function shutdownTorchServer(): Promise<string | null> {
  try {
    const ss = await findServer("demucs_torch_server");
    const baseUrl = `http://127.0.0.1:${ss.port}`;

    if (!(await healthCheck(baseUrl))) return null;

    console.log(`[TorchServer] Shutting down server at ${baseUrl}...`);
    await fetch(`${baseUrl}/api/shutdown`, {
      method: "POST",
      signal: AbortSignal.timeout(5000),
    });
    // Wait briefly for the process to exit
    await new Promise((r) => setTimeout(r, 500));
    return baseUrl;
  } catch (e) {
    console.warn(
      "[TorchServer] Shutdown signal failed (server may already be gone):",
      (e as Error).message,
    );
    return null;
  }
}
