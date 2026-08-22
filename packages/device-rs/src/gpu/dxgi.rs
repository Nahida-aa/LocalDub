//! DXGI 实时显存探测（仅 Windows）。
//!
//! 通过 `IDXGIAdapter3::QueryVideoMemoryInfo` 读取每个 adapter 的
//! `DXGI_MEMORY_SEGMENT_GROUP_LOCAL`（fast pool）与
//! `DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL`（slow pool）的 Budget / CurrentUsage。
//!
//! 这是唯一能在 Windows 上**精确区分** APU 的 LOCAL（carveout）
//! 与 NON_LOCAL（系统内存 GTT）的源——Vulkan 的 DEVICE_LOCAL 在 APU 上会混入 GTT，
//! 规范没有提供 LOCAL/NON_LOCAL 标志。该源全厂商、全 GPU 实时生效。
#![cfg(windows)]

use serde::Serialize;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3,
    IDXGIFactory3,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::Interface;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DxgiMemorySegment {
    /// 该段（local/non-local）预算字节数
    pub budget_bytes: u64,
    /// 当前使用字节数（全系统占用）
    pub current_usage_bytes: u64,
    pub available_for_reservation_bytes: u64,
    pub current_reservation_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DxgiAdapterProbe {
    pub index: u32,
    pub description: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub luid_low: u32,
    pub luid_high: i32,
    /// fast pool：GDDR（dGPU）或 carveout（APU）
    pub local: DxgiMemorySegment,
    /// slow pool：PCIe GTT（dGPU）或系统内存 GTT（APU）
    pub non_local: DxgiMemorySegment,
}

fn to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn adapter_desc_to_string(desc: &DXGI_ADAPTER_DESC1) -> String {
    let desc = &desc.Description;
    let len = desc.iter().position(|&c| c == 0).unwrap_or(desc.len());
    String::from_utf16_lossy(&desc[..len])
}

pub fn probe_dxgi() -> Vec<DxgiAdapterProbe> {
    let mut probes = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory3>() else {
            return probes;
        };
        for index in 0u32.. {
            let Ok(adapter1) = factory.EnumAdapters1(index) else {
                break; // DXGI_ERROR_NOT_FOUND → 枚举结束
            };
            let Ok(desc) = adapter1.GetDesc1() else {
                continue;
            };
            let Ok(adapter3) = adapter1.cast::<IDXGIAdapter3>() else {
                continue;
            };
            let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            let mut non_local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if adapter3
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)
                .is_err()
            {
                continue;
            }
            if adapter3
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut non_local)
                .is_err()
            {
                continue;
            }
            probes.push(DxgiAdapterProbe {
                index,
                description: adapter_desc_to_string(&desc),
                vendor_id: desc.VendorId,
                device_id: desc.DeviceId,
                luid_low: desc.AdapterLuid.LowPart,
                luid_high: desc.AdapterLuid.HighPart,
                local: DxgiMemorySegment {
                    budget_bytes: local.Budget,
                    current_usage_bytes: local.CurrentUsage,
                    available_for_reservation_bytes: local.AvailableForReservation,
                    current_reservation_bytes: local.CurrentReservation,
                },
                non_local: DxgiMemorySegment {
                    budget_bytes: non_local.Budget,
                    current_usage_bytes: non_local.CurrentUsage,
                    available_for_reservation_bytes: non_local.AvailableForReservation,
                    current_reservation_bytes: non_local.CurrentReservation,
                },
            });
        }
    }
    probes
}

/// 便于人读的摘要（GB）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DxgiProbeSummary {
    pub description: String,
    pub local_total_gb: f64,
    pub local_used_gb: f64,
    pub non_local_total_gb: f64,
    pub non_local_used_gb: f64,
}

impl From<&DxgiAdapterProbe> for DxgiProbeSummary {
    fn from(p: &DxgiAdapterProbe) -> Self {
        DxgiProbeSummary {
            description: p.description.clone(),
            local_total_gb: to_gb(p.local.budget_bytes),
            local_used_gb: to_gb(p.local.current_usage_bytes),
            non_local_total_gb: to_gb(p.non_local.budget_bytes),
            non_local_used_gb: to_gb(p.non_local.current_usage_bytes),
        }
    }
}
