import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { $ } from "bun";
import { emitLog, probeDuration, separateDir } from "@repo/core/stages/utils/utils";
import { setStage } from "@repo/core/context/context";
import { DemucsCliArgs } from "./cli_types";
import { DEMUCS_MODEL_DIR } from "@repo/config/path/models";
import { REPO_ROOT } from "@repo/config/root";
import { log } from "@repo/util/log";

function findLibtorchPath(): string | null {
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

/** 确保 demucs-burn-${backend} 二进制已构建（缺失时自动编译），返回 bin 路径。 */
async function ensureDemucsBin(taskDir: string, binName: string): Promise<string> {
  const binPath = join(REPO_ROOT, "target", "release", binName);
  if (existsSync(binPath)) return binPath;

  const crateDir = join(REPO_ROOT, "packages", "separate", "demucs_burn");
  // Cargo.toml 中每个 bin 有各自 required-features（tch / cubecl-wgpu / ...），
  // 按 bin 后缀派生对应 feature 并以 --no-default-features 关闭默认的 cubecl-wgpu。
  const backend = binName.replace("demucs-burn-", "");
  const feature = backend === "tch" ? "tch" : `cubecl-${backend}`;
  let task = demucsBuildTasks.get(binName);
  if (!task) {
    task = (async () => {
      log(`${binName} 未构建，自动编译...`);
      const build =
        await $`cargo build --release --no-default-features --features ${feature} -p demucs-burn --bin ${binName}`
          .cwd(crateDir)
          .nothrow();
      if (build.exitCode !== 0) {
        demucsBuildTasks.delete(binName);
        throw new Error(`${binName} 编译失败 (exit ${build.exitCode}):\n${build.stderr}`);
      }
      if (!existsSync(binPath)) {
        demucsBuildTasks.delete(binName);
        throw new Error(
          `${binName} 编译完成但找不到产物: ${binPath}\n` +
            `请检查 Cargo.toml 中 ${binName} 的 required-features 是否满足。`,
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

  const durationS = probeDuration(audioPath);
  if (durationS > 0) {
    log(`RTF ${(elapsedSec / durationS).toFixed(3)}`);
  }
}
