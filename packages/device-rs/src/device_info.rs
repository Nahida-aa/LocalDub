use serde::{Deserialize, Serialize};
use specta::Type;

use crate::gpu::get_gpu_info;
use crate::gpu::GpuInfo;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub platform: PlatformInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub gpu: Vec<GpuInfo>,
    pub ort: OrtInfo,
}

/// 采集设备信息 (镜像 TS `packages/device/src/device-info.ts::getDeviceInfo`)。
///
/// 差异: ORT 不链 onnxruntime, `ort` 报 `version="unknown"`, `backends=[]`。
pub fn get_device_info() -> DeviceInfo {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();

    // CPU: 第一颗核的 model/frequency, cores = 逻辑核数。
    let cpus = sys.cpus();
    let cpu_model = cpus.first().map(|c| c.brand().to_string()).unwrap_or_else(|| "unknown".into());
    let speed_mhz = cpus.first().map(|c| c.frequency() as f64).unwrap_or(0.0);
    let cores = cpus.len() as u32;

    // 内存 (字节)。sysinfo 0.30 起 `total_memory/free_memory` 直接返回字节。
    let total_bytes = sys.total_memory();
    let free_bytes = sys.free_memory();
    let heap_bytes = process_heap_used_bytes();

    let arch = std::env::consts::ARCH.to_string();
    let runtime = "rust".to_string();
    let runtime_version = format!("v{}", env!("CARGO_PKG_VERSION"));

    DeviceInfo {
        platform: PlatformInfo {
            os: std::env::consts::OS.to_string(),
            arch,
            release: sysinfo::System::kernel_version().unwrap_or_default(),
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            runtime,
            runtime_version,
            node_version: None,
        },
        cpu: CpuInfo {
            model: cpu_model,
            cores,
            speed_mhz,
        },
        memory: MemoryInfo {
            total: fmt_bytes(total_bytes),
            free: fmt_bytes(free_bytes),
            process_heap_used: fmt_bytes(heap_bytes),
        },
        gpu: get_gpu_info(),
        ort: OrtInfo {
            version: "unknown".into(),
            backends: Vec::new(),
        },
    }
}

/// 进程占用内存 (字节)。镜像 TS `process.memoryUsage().heapUsed`。
///
/// Rust 无 V8 heap 概念, 用当前进程 RSS (Linux /proc/self/statm) 代替。
fn process_heap_used_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/proc/self/statm") {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                // "size rss shared text lib data dt" (pages)。rss = 第 2 列。
                if let Some(rss_page) = buf.split_whitespace().nth(1) {
                    if let Ok(rss) = rss_page.parse::<u64>() {
                        return rss * 4096; // 4K 页
                    }
                }
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// 镜像 TS `fmtBytes`: `到 GB 保留 1 位小数 + " GB"`。
fn fmt_bytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1024.0_f64.powi(3))
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub release: String,
    pub hostname: String,
    pub runtime: String,
    pub runtime_version: String,
    #[serde(skip_serializing_if = "Option::is_none")] // | undefined
    pub node_version: Option<String>, // T | None(null)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub model: String,
    pub cores: u32,
    #[serde(rename = "speedMHz")]
    pub speed_mhz: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total: String,
    pub free: String,
    pub process_heap_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrtInfo {
    pub version: String,
    pub backends: Vec<OrtBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrtBackend {
    pub name: String,
    pub bundled: bool,
}
