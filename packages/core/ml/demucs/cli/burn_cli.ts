import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { $ } from "bun";
import { emitLog, separateDir } from "@repo/core/stages/utils/utils";
import { probeDurationMs } from "@repo/core/utils/ffmpeg";
import { setStage } from "@repo/core/context/context";
import { DemucsCliArgs } from "./cli_types";
import { DEMUCS_MODEL_DIR } from "@repo/config/path/models";
import { REPO_ROOT } from "@repo/config/root";
import { DATA_DIR } from "@repo/config/path/paths";
import { log } from "@repo/util/log";

function findLibtorchPath(): string | null {
  // release 下载: libtorch_cpu.so 平铺在 data/bin (与二进制同目录)
  const dlLib = join(DATA_DIR, "bin");
  if (existsSync(join(dlLib, "libtorch_cpu.so"))) return dlLib;
  // 兼容旧源码构建目录: target/release/build/torch-sys-*/out/libtorch/libtorch/lib
  const buildDir = join(REPO_ROOT, "target", "release", "build");
  if (!existsSync(buildDir)) return null;
  for (const dir of readdirSync(buildDir)) {
    if (!dir.startsWith("torch-sys-")) continue;
    const libDir = join(buildDir, dir, "out", "libtorch", "libtorch", "lib");
    if (existsSync(join(libDir, "libtorch_cpu.so"))) return libDir;
  }
  return null;
}

const demucsBuildTasks = new Map<string, Promise<string>>();

/**
 * 确保 demucs-burn-${backend} 就绪，返回 bin 路径。
 *
 * demucs-burn 已随迁移改为从 vox-lab GitHub Release 下载（data/bin），不再本地源码编译：
 * - tch/wgpu: 委托 Rust `cli env ensure demucs_burn_{tch,wgpu}_bin`（release 下载，见
 *   `packages/core/src/cmd/env/items.rs` 的 DEMUCS_BURN_* spec）。
 * - 其余后端（cpu/cuda/vulkan）无发布资产，直接报错（与 Rust separate 一致）。
 */
async function ensureDemucsBin(taskDir: string, binName: string): Promise<string> {
  const backend = binName.replace("demucs-burn-", "");
  if (backend !== "tch" && backend !== "wgpu") {
    throw new Error(
      `demucs-burn 后端 ${backend} 暂无发布资产。请切换 separate.runtime/device 到 burn-tch (tch) 或 burn+webgpu (wgpu)。`,
    );
  }

  const key = `demucs_burn_${backend}_bin`;
  const binPath = join(DATA_DIR, "bin", binName);
  if (existsSync(binPath)) return binPath;

  let task = demucsBuildTasks.get(binName);
  if (!task) {
    task = (async () => {
      log(`${binName} 未下载，调用 Rust env ensure (${key})...`);
      const ensure = await $`cargo run -p cli -- env ensure ${key}`.cwd(REPO_ROOT).nothrow();
      if (ensure.exitCode !== 0 || !existsSync(binPath)) {
        demucsBuildTasks.delete(binName);
        throw new Error(
          `${binName} 下载/校验失败 (exit ${ensure.exitCode}):\n${ensure.stderr}\n` +
            `请手动执行: cargo run -p cli -- env ensure ${key}`,
        );
      }
      return binPath;
    })();
    demucsBuildTasks.set(binName, task);
  }
  return task;
}

export async function separateBurn({
  taskDir,
  audioPath,
  device,
  backend,
}: DemucsCliArgs & {
  backend?: string;
}) {
  backend ??= device === "cpu" ? "tch" : "wgpu";
  const binName = `demucs-burn-${backend}`;
  const binPath = await ensureDemucsBin(taskDir, binName);
  const modelPath = join(DEMUCS_MODEL_DIR, "htdemucs_ft.safetensors");

  if (!existsSync(modelPath)) {
    throw new Error(
      `Model not cached at ${modelPath}\n` +
        "Run demucs-burn-wgpu first to download it." +
        " The model will be downloaded automatically on first run.",
    );
  }

  if (!existsSync(audioPath)) {
    throw new Error("audio_source.wav not found");
  }

  const sepDir = separateDir(taskDir);
  mkdirSync(sepDir, { recursive: true });

  log(`runtime=${binName} device=${device} binary=${binPath}`);

  const env: Record<string, string> = { ...process.env } as Record<string, string>;
  if (backend === "tch") {
    const libtorchLib = findLibtorchPath();
    if (!libtorchLib) {
      throw new Error("libtorch not found. Build tch binary first.");
    }
    env.LD_LIBRARY_PATH = [libtorchLib, env.LD_LIBRARY_PATH].filter(Boolean).join(":");
  }

  const t0 = performance.now();
  await new Promise<void>((resolve, reject) => {
    const proc = spawn(binPath, [audioPath, sepDir], { env });
    let stderr = "";

    let lastPct = -1;
    proc.stdout?.on("data", (chunk: Buffer) => {
      const lines = chunk.toString().split("\n");
      for (const line of lines) {
        const m = line.match(/\((\s*\d+(?:\.\d+)?)%\)/);
        if (m) {
          const pct = Math.min(100, Math.max(0, Math.round(Number(m[1]))));
          if (pct === lastPct) continue;
          lastPct = pct;
          setStage(taskDir, "separate", {
            progress: pct,
            last_message: `Separating ${pct}%`,
          });
        }
      }
    });

    proc.stderr?.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });

    proc.on("error", (e) => {
      reject(new Error(`Burn separate failed to spawn: ${e.message}`));
    });

    proc.on("close", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`Burn separate failed (${code}): ${stderr.slice(-300)}`));
    });
  });
  const elapsedSec = (performance.now() - t0) / 1000;

  log(`Processed in ${elapsedSec.toFixed(1)}s`);

  const stemNames = ["drums", "bass", "other", "vocals"] as const;
  for (const name of stemNames) {
    const p = join(sepDir, `target_${stemNames.indexOf(name)}_${name}.wav`);
    if (!existsSync(p)) {
      log(`WARN: ${p} not found`);
    }
  }

  const durationMs = probeDurationMs(audioPath);
  if (durationMs > 0) {
    log(`RTF ${(elapsedSec / (durationMs / 1000)).toFixed(3)}`);
  }
}
