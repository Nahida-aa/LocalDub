import type { GpuInfo } from "./types.ts";
import { run } from "../utils.ts";

/**
 * Windows 注册表显存探测源。
 *
 * 读取 `HKLM\...\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\<0xxx>`
 * 下显示适配器子键的 `HardwareInformation.qwMemorySize`（64 位真实显存，bytes）。
 *
 * 注意：
 * - 不要用 Win32_VideoController.AdapterRAM（32 位有符号整数，>4GB 会溢出为负）。
 * - 该源为静态兜底：仅提供 total，used 为 null（由账本 / DXGI 等实时源覆盖）。
 * - 优先级最低，仅在其他源缺失 total 时填充（见 gpu.ts 合并逻辑）。
 */

const GPU_CLASS_KEY =
  "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}";
const MAX_SUBKEYS = 16; // 0000 ~ 000F，足够覆盖多 GPU / 多显示器适配器

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function queryValue(subKey: string, valueName: string): string | null {
  const out = run(`reg query "${subKey}" /v "${valueName}"`, 3000);
  if (!out) return null;
  const re = new RegExp(`${escapeRegExp(valueName)}\\s+REG_\\w+\\s+(.+)`);
  for (const line of out.split("\n")) {
    const m = line.match(re);
    if (m) return m[1].trim();
  }
  // 兜底：值名存在但格式不同，取该行最后一个 token
  const line = out.split("\n").find((l) => l.includes(valueName));
  if (!line) return null;
  const tokens = line.trim().split(/\s+/);
  return tokens[tokens.length - 1] ?? null;
}

function parseQword(raw: string | null): bigint | null {
  if (!raw) return null;
  try {
    return BigInt(raw); // 支持 0x hex 与十进制
  } catch {
    return null;
  }
}

function guessVendor(name: string): GpuInfo["vendor"] {
  const lower = name.toLowerCase();
  if (lower.includes("nvidia")) return "nvidia";
  if (lower.includes("radeon") || lower.includes("amd")) return "amd";
  if (lower.includes("intel")) return "intel";
  return "unknown";
}

export function tryWindowsRegistry(): GpuInfo[] {
  if (process.platform !== "win32") return [];
  const gpus: GpuInfo[] = [];

  for (let i = 0; i < MAX_SUBKEYS; i++) {
    const subKey = `${GPU_CLASS_KEY}\\${String(i).padStart(4, "0")}`;
    const totalBytes = parseQword(queryValue(subKey, "HardwareInformation.qwMemorySize"));
    if (totalBytes == null || totalBytes <= 0n) continue;
    // 0xFFFFFFFFFFFFFFFF 表示未知 / 无显存信息
    if (totalBytes === BigInt("0xFFFFFFFFFFFFFFFF")) continue;

    const totalGB = Number(totalBytes) / 1024 ** 3;
    if (totalGB <= 0) continue;

    const name = queryValue(subKey, "HardwareInformation.AdapterString") ?? `GPU ${i}`;
    gpus.push({
      name,
      architecture: undefined,
      driverVersion: "",
      temperature: 0,
      gpuPercent: 0,
      vram: {
        percent: 0,
        total: totalGB,
        used: null,
        type: "dedicated",
      },
      vendor: guessVendor(name),
      capabilities: {
        webgpu: false,
        vulkan: false,
        cuda: false,
        rocm: false,
        directml: false,
        mps: false,
        openvino: false,
      },
    });
  }

  return gpus;
}
