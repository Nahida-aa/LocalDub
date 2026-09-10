//! GPU 采集 (镜像 TS `packages/device/src/gpu/gpu.ts` 与三个来源解析器)。
//!
//! 三来源:
//! - [`try_cuda_smi`]: nvidia-smi (name/temp/util/mem)
//! - [`try_rocm_smi`]: rocm-smi 系列 (APU VRAM/GTT + GFX version + HSA override)
//! - [`try_vulkan_info`]: vulkaninfo (json 优先, 回退文本解析, heap 视角)
//!
//! 合并规则对齐 TS `getGpuInfo`: 按 `vendor|normName` 去重, 不同来源互相补字段。

use super::{
    try_vulkan_info, Capabilities, GpuInfo, Vendor, VramInfo, VramType,
    is_linux, norm_name,
};
use crate::collect::run_with_timeout;

/// 去重 key (vendor|normName 镜像 TS)。Vendor 未 derive Debug, 手写映射。
fn vendor_key(v: &Vendor) -> &'static str {
    match v {
        Vendor::Amd => "amd",
        Vendor::Nvidia => "nvidia",
        Vendor::Intel => "intel",
        Vendor::Unknown => "unknown",
    }
}

/// 入口: 合并三来源 GPU 信息 (镜像 TS `getGpuInfo`)。
pub fn get_gpu_info() -> Vec<GpuInfo> {
    let mut sources: Vec<Vec<GpuInfo>> = Vec::new();
    if is_linux() {
        let rocm = try_rocm_smi();
        if !rocm.is_empty() {
            sources.push(rocm);
        }
    }
    let cuda = try_cuda_smi();
    if !cuda.is_empty() {
        sources.push(cuda);
    }
    let vulkan = try_vulkan_info();
    if !vulkan.is_empty() {
        sources.push(vulkan);
    }

    // 按 vendor|normName 去重, 不同来源补字段 (同 TS gpu.ts 合并循环)。
    let mut seen: Vec<GpuInfo> = Vec::new();
    for gpu in sources.into_iter().flatten() {
        let key = format!("{}|{}", vendor_key(&gpu.vendor), norm_name(&gpu.name));
        if let Some(existing) = seen.iter_mut().find(|e| {
            format!("{}|{}", vendor_key(&e.vendor), norm_name(&e.name)) == key
        }) {
            merge_gpu(existing, gpu);
        } else {
            seen.push(gpu);
        }
    }
    seen
}

/// 合并同 key GPU (镜像 TS 合并循环的各 if)。
fn merge_gpu(existing: &mut GpuInfo, gpu: GpuInfo) {
    if gpu.vulkan_heaps.is_some() && existing.vulkan_heaps.is_none() {
        existing.vulkan_heaps = gpu.vulkan_heaps;
    }
    if gpu.vram.total.is_some() && existing.vram.total.is_none() {
        existing.vram.total = gpu.vram.total;
    }
    if gpu.temperature > 0.0 && existing.temperature == 0.0 {
        existing.temperature = gpu.temperature;
    }
    if gpu.gpu_percent > 0.0 && existing.gpu_percent == 0.0 {
        existing.gpu_percent = gpu.gpu_percent;
    }
    if gpu.vram.percent > 0.0 && existing.vram.percent == 0.0 {
        existing.vram.percent = gpu.vram.percent;
    }
    if gpu.vram.used.is_some() && existing.vram.used.is_none() {
        existing.vram.used = gpu.vram.used;
    }
    if gpu.vram.r#type != Some(VramType::Unknown)
        && (existing.vram.r#type.is_none() || existing.vram.r#type == Some(VramType::Unknown))
    {
        existing.vram.r#type = gpu.vram.r#type;
    }
    if gpu.vram.gtt.is_some() && existing.vram.gtt.is_none() {
        existing.vram.gtt = gpu.vram.gtt;
    }
}

/// nvidia-smi (镜像 TS `tryCudaSmi`): CSV 行 name,temp,gpuPct,totalMiB,usedMiB。
fn try_cuda_smi() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    let out = run_with_timeout(
        "nvidia-smi",
        &[
            "--query-gpu=name,temperature.gpu,utilization.gpu,memory.total,memory.used",
            "--format=csv,noheader,nounits",
        ],
        5000,
    );
    if out.is_empty() {
        return gpus;
    }
    let caps = Capabilities {
        cuda: true,
        rocm: false,
        mps: false,
        webgpu: true,
        vulkan: true,
        directml: false,
        openvino: false,
    };
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let parts: Vec<&str> = line.split(", ").map(|s| s.trim()).collect();
        if parts.len() < 5 {
            continue;
        }
        let name = parts[0].to_string();
        let temp = parts[1].parse::<f64>().unwrap_or(0.0);
        let gpu_pct = parts[2].parse::<f64>().unwrap_or(0.0);
        let total_mib = parts[3].parse::<f64>().unwrap_or(0.0);
        let used_mib = parts[4].parse::<f64>().unwrap_or(0.0);
        let vram_total_gb = total_mib / 1024.0;
        let vram_used_gb = used_mib / 1024.0;
        let vram_pct = if total_mib > 0.0 {
            (used_mib / total_mib * 100.0).round()
        } else {
            0.0
        };
        gpus.push(GpuInfo {
            name,
            architecture: None,
            driver_version: String::new(),
            temperature: temp,
            gpu_percent: gpu_pct,
            vram: VramInfo {
                percent: vram_pct,
                total: Some(vram_total_gb),
                used: Some(vram_used_gb),
                r#type: Some(VramType::Dedicated),
                reserved: None,
                gtt: None,
            },
            vendor: Vendor::Nvidia,
            capabilities: caps.clone(),
            gfx_version: None,
            hsa_override_gfx: None,
            vulkan_heaps: None,
            op_probes: None,
        });
    }
    gpus
}

/// rocm-smi (镜像 TS `tryRocmSmi`): 输出 + meminfo + productname + 包管理器版本。
fn try_rocm_smi() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    let caps = Capabilities {
        cuda: false,
        rocm: true,
        mps: false,
        webgpu: true,
        vulkan: true,
        directml: false,
        openvino: false,
    };
    let smi = run_with_timeout("rocm-smi", &[], 3000);
    if smi.is_empty() {
        return gpus;
    }
    let driver_ver = rocm_version_from_package_manager().unwrap_or_else(|| "unknown".into());
    let hsa_override = std::env::var("HSA_OVERRIDE_GFX_VERSION").ok();

    // meminfo: VRAM/GTT 总量 (APU 有 GTT, dGPU 无)。镜像 TS 正则 `\s+Total Memory \(B\):\s*(\d+)`。
    let meminfo = run_with_timeout("rocm-smi", &["--showmeminfo", "all"], 3000);
    let mut vram_total_gb: Option<f64> = None;
    let mut gtt_total_gb: Option<f64> = None;
    let mut is_apu = false;
    for line in meminfo.lines() {
        // VRAM Total Memory (B): 4294967296
        if let Some(cap) = line.trim().split("VRAM Total Memory (B):").nth(1) {
            if let Some(gb) = parse_bytes_gb(cap.trim()) {
                vram_total_gb = Some(gb);
            }
        }
        // GTT Total Memory (B): 14590562304 → APU
        if let Some(cap) = line.trim().split("GTT Total Memory (B):").nth(1) {
            if let Some(gb) = parse_bytes_gb(cap.trim()) {
                gtt_total_gb = Some(gb);
                is_apu = true;
            }
        }
    }

    let pn = run_with_timeout("rocm-smi", &["--showproductname"], 3000);
    // 每个 GPU 的 Card Series (镜像 TS `GPU\[${id}\]\s*:\s*Card Series:\s*(.+)`)。
    let mut card_series: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // GFX Version 取全文首个 (镜像 TS `pn.match(/GFX Version:/i)` 无 g flag)。
    let mut gfx_ver = String::new();
    for line in pn.lines() {
        let line = line.trim();
        if let Some(id) = line.split('[').nth(1).and_then(|r| r.split(']').next()) {
            if let Some(name) = line.split("Card Series:").nth(1) {
                card_series.insert(
                    id.trim().to_string(),
                    name.trim().to_string(),
                );
            }
        }
        if gfx_ver.is_empty() {
            if let Some(g) = line.split("GFX Version:").nth(1) {
                gfx_ver = g.trim().to_string();
            }
        }
    }

    for line in smi.lines() {
        if !line.starts_with(char::is_numeric) {
            continue;
        }
        let trimmed = line.trim();
        let id = trimmed.split_whitespace().next().unwrap_or_default().to_string();
        if id.parse::<u64>().is_err() {
            continue;
        }
        let temp = trimmed
            .split("°C")
            .next()
            .map(|s| s.rsplit(' ').next())
            .flatten()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        // 百分比: 最后两个分别 vramPct / gpuPct (TS 取 len-2 与 len-1)。
        let pcts: Vec<f64> = regex_pcts(trimmed);
        let vram_pct = *pcts.get(pcts.len().saturating_sub(2)).unwrap_or(&0.0);
        let gpu_pct = *pcts.last().unwrap_or(&0.0);

        let gpu_name = card_series
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("GPU {id}"));

        gpus.push(GpuInfo {
            name: if gpu_name.is_empty() {
                format!("GPU {id}")
            } else {
                gpu_name
            },
            architecture: gfx_ver_name(&gfx_ver),
            driver_version: driver_ver.clone(),
            temperature: temp,
            vram: VramInfo {
                percent: vram_pct,
                total: vram_total_gb,
                used: None,
                r#type: Some(if is_apu { VramType::Shared } else { VramType::Dedicated }),
                reserved: None,
                gtt: gtt_total_gb,
            },
            gpu_percent: gpu_pct,
            gfx_version: if gfx_ver.is_empty() { None } else { Some(gfx_ver.clone()) },
            hsa_override_gfx: hsa_override.clone(),
            vendor: Vendor::Amd,
            capabilities: caps.clone(),
            vulkan_heaps: None,
            op_probes: None,
        });
    }

    // 无行解析到时 fallback: 单个 Unknown (TS 末尾 fallback)。
    if gpus.is_empty() {
        let fallback_pn = run_with_timeout("rocm-smi", &["--showproductname"], 3000);
        let name = fallback_pn
            .lines()
            .find_map(|l| l.split("Card Series:").nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown".into());
        gpus.push(GpuInfo {
            name,
            architecture: None,
            driver_version: driver_ver,
            temperature: 0.0,
            vram: VramInfo {
                percent: 0.0,
                total: None,
                used: None,
                r#type: None,
                reserved: None,
                gtt: None,
            },
            gpu_percent: 0.0,
            vendor: Vendor::Amd,
            capabilities: caps,
            gfx_version: None,
            hsa_override_gfx: hsa_override,
            vulkan_heaps: None,
            op_probes: None,
        });
    }

    gpus
}

/// 解析 rocm-smi meminfo 的字节字段并转 GB (镜像 TS `parseInt(...) / 1024**3`)。
fn parse_bytes_gb(s: &str) -> Option<f64> {
    let bytes = s.trim().parse::<u64>().ok()?;
    if bytes == 0 {
        return None;
    }
    Some(bytes as f64 / 1024.0_f64.powi(3))
}

/// 提取字符串中所有百分比数 (镜像 TS `[...line.matchAll(/(\d+)%/g)]`)。
fn regex_pcts(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'%' {
                if let Ok(v) = s[start..i].parse::<f64>() {
                    out.push(v);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// gfxVersion -> architecture (镜像 TS `GFX_ARCH_MAP`)。
fn gfx_ver_name(gfx: &str) -> Option<String> {
    let name = match gfx {
        "gfx1010" | "gfx1011" | "gfx1012" => "RDNA 1",
        "gfx1030" | "gfx1031" | "gfx1032" | "gfx1034" | "gfx1035" | "gfx1036" => "RDNA 2",
        "gfx1100" | "gfx1101" | "gfx1102" | "gfx1103" => "RDNA 3",
        "gfx1150" | "gfx1151" => "RDNA 3.5",
        "gfx1200" | "gfx1201" => "RDNA 4",
        _ => return None,
    };
    Some(name.into())
}

/// 包管理器查询 rocm-core 版本 (镜像 TS `getRocmVersionFromPackageManager`)。
fn rocm_version_from_package_manager() -> Option<String> {
    let pacman = run_with_timeout("pacman", &["-Q", "rocm-core"], 3000);
    if let Some(v) = pacman.split_whitespace().nth(1) {
        return Some(v.to_string());
    }
    let dpkg = run_with_timeout("dpkg", &["-l", "rocm-core"], 3000);
    // "ii  rocm-core  x.y.z  ..." 第 4 字段是版本 (TS 正则宽松匹配 [\d.]+)
    if let Some(field4) = dpkg.split_whitespace().nth(3) {
        return Some(field4.to_string());
    }
    let rpm = run_with_timeout("rpm", &["-q", "rocm-core"], 3000);
    if let Some(v) = rpm
        .split_once("rocm-core-")
        .and_then(|(_, rest)| rest.split('.').next())
    {
        return Some(format!("{v}.0"));
    }
    let _ = &rpm;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_name_strips_suffix() {
        assert_eq!(norm_name("AMD Radeon Graphics (RADV PHOENIX)"), "AMD Radeon Graphics");
        assert_eq!(norm_name("NVIDIA GeForce RTX 4070"), "NVIDIA GeForce RTX 4070");
    }

    #[test]
    fn regex_pcts_collects_all() {
        // 与 TS `/(\d+)%/g` 同语义: `25.0%` 只匹配 `0%` (整数部分后是 `.` 不是 `%`)。
        let s = "GPU[0]: 25.0% utilization, 30% VRAM";
        assert_eq!(regex_pcts(s), vec![0.0, 30.0]);
    }

    #[test]
    fn gfx_ver_name_maps_arch() {
        assert_eq!(gfx_ver_name("gfx1100").as_deref(), Some("RDNA 3"));
        assert_eq!(gfx_ver_name("gfx1200").as_deref(), Some("RDNA 4"));
        assert_eq!(gfx_ver_name("gfx9999"), None);
    }
}