//! vulkaninfo 解析 (镜像 TS `packages/device/src/gpu/VulkanInfo.ts`)。
//!
//! json 输出优先 (`vulkaninfo --json`), 失败回退文本输出 (`vulkaninfo`)。
//! 提取每个物理设备的 deviceName / vendorID / memory heaps, 计算
//! `vulkanHeaps` (deviceLocal / hostVisible) 与 vram 视角。

use super::{Capabilities, GpuInfo, Vendor, VramInfo, VramType, VulkanHeaps, is_linux};
use crate::collect::run_with_timeout;

/// 入口 (镜像 TS `tryVulkanInfo`)。
pub fn try_vulkan_info() -> Vec<GpuInfo> {
    // --- json 优先 ---
    let json_output = run_with_timeout("vulkaninfo", &["--json"], 5000);
    let from_json = parse_vulkan_json(&json_output);
    if !from_json.is_empty() {
        return from_json;
    }

    // --- 文本回退 ---
    let text_output = run_with_timeout("vulkaninfo", &[], 10000);
    parse_vulkan_text(&text_output)
}

/// 解析 `vulkaninfo --json` (mirror TS json 分支)。
fn parse_vulkan_json(json_output: &str) -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    if json_output.is_empty() {
        return gpus;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(json_output) else {
        return gpus;
    };
    let Some(devices) = data.get("VkPhysicalDevices").and_then(|v| v.as_array()) else {
        return gpus;
    };
    for dev in devices {
        let props = dev.get("VkPhysicalDeviceProperties").cloned().unwrap_or_default();
        let mem = dev
            .get("VkPhysicalDeviceMemoryProperties")
            .cloned()
            .unwrap_or_default();

        let mut device_local_gb = 0.0_f64;
        let mut host_visible_gb = 0.0_f64;
        if let Some(heaps) = mem.get("memoryHeaps").and_then(|v| v.as_array()) {
            for heap in heaps {
                let size_gb = heap
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .map(|b| b as f64 / 1024.0_f64.powi(3))
                    .unwrap_or(0.0);
                let flags = heap.get("flags").cloned().unwrap_or_default();
                if flags.get("deviceLocal").and_then(|v| v.as_bool()).unwrap_or(false) {
                    device_local_gb = device_local_gb.max(size_gb);
                } else if flags.get("hostVisible").and_then(|v| v.as_bool()).unwrap_or(false) {
                    host_visible_gb = host_visible_gb.max(size_gb);
                }
            }
        }
        let vram_total_gb = device_local_gb;
        let vendor = vendor_from_id(
            props
                .get("vendorID")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        );
        let is_integrated = host_visible_gb > 0.0 && device_local_gb > 0.0;
        let gtt_gb = if host_visible_gb > 0.0 { Some(host_visible_gb) } else { None };
        let name = props
            .get("deviceName")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Vulkan GPU")
            .to_string();
        let driver_version = props
            .get("driverVersion")
            .map(|v| format!("{v}"))
            .unwrap_or_default();
        gpus.push(GpuInfo {
            name,
            architecture: None,
            driver_version,
            temperature: 0.0,
            gpu_percent: 0.0,
            vram: VramInfo {
                percent: 0.0,
                total: if vram_total_gb > 0.0 { Some(vram_total_gb) } else { None },
                used: Some(0.0),
                r#type: Some(if is_integrated { VramType::Shared } else { VramType::Dedicated }),
                reserved: None,
                gtt: gtt_gb,
            },
            vulkan_heaps: if device_local_gb > 0.0 || host_visible_gb > 0.0 {
                Some(VulkanHeaps {
                    device_local: device_local_gb,
                    host_visible: host_visible_gb,
                })
            } else {
                None
            },
            vendor,
            capabilities: caps_for(vendor),
            gfx_version: None,
            hsa_override_gfx: None,
            op_probes: None,
        });
    }
    gpus
}

/// 解析 vulkaninfo 文本输出 (mirror TS 文本分支)。
fn parse_vulkan_text(text_output: &str) -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    if text_output.is_empty() {
        return gpus;
    }
    // 按 "VkPhysicalDeviceProperties:" 切段, 跳过首个 header+layers 段。
    let sections: Vec<&str> = text_output.split("VkPhysicalDeviceProperties:").collect();
    for (i, section) in sections.iter().enumerate().skip(1) {
        // 从本段起点到下一段起点的完整区域。
        let start = text_output.find(section).unwrap_or(0);
        let end = if i + 1 < sections.len() {
            text_output
                .find(sections[i + 1])
                .unwrap_or(text_output.len())
        } else {
            text_output.len()
        };
        let full_block = &text_output[start..end];

        let Some(name) = find_field(section, "deviceName") else {
            continue;
        };
        let vendor_id = find_field(full_block, "vendorID")
            .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let vendor = vendor_from_id(vendor_id);

        // memory heaps: memoryHeaps[i]: size = N ... flags.
        let mut device_local_gb = 0.0_f64;
        let mut host_visible_gb = 0.0_f64;
        let mem_start = full_block.find("VkPhysicalDeviceMemoryProperties:");
        if let Some(mem_start) = mem_start {
            let mem_block = &full_block[mem_start..];
            // 逐 heap: `memoryHeaps[i]:` ... size 行 ... flags ... (到下一个 heap 块或 memoryTypes)。
            // 兼容新旧 vulkaninfo 文本格式 (size 行多空格/hex 后缀; TS 正则 `size\s*=\s*(\d+)`)。
            let mut pos = 0;
            while let Some(rel) = mem_block[pos..].find("memoryHeaps[") {
                let before_next = &mem_block[pos..];
                // 本 heap 起始 (含 "memoryHeaps[i]:" 行)。
                let after_marker = &before_next[rel + 12..];
                let end = after_marker.find("memoryHeaps[").unwrap_or(0);
                // heap 块边界: 到下一个 memoryHeaps 或 memoryTypes/段尾。
                let heap_size_limit = mem_block
                    .find("memoryTypes")
                    .unwrap_or(mem_block.len())
                    .max(pos + rel);
                let heap_block = if end > 0 {
                    &mem_block[pos + rel..(pos + rel + 12 + end).min(heap_size_limit)]
                } else {
                    &mem_block[pos + rel..heap_size_limit]
                };
                // size 行: `size   = 6295175168 (...)` 取首个数字。
                let mut size_bytes: Option<u64> = None;
                for line in heap_block.lines() {
                    let t = line.trim();
                    if let Some(rest) = t.strip_prefix("size") {
                        if let Some(eq) = rest.find('=') {
                            let digits: String = rest[eq + 1..]
                                .chars()
                                .skip_while(|c| c.is_whitespace())
                                .take_while(|c| c.is_ascii_digit())
                                .collect();
                            if !digits.is_empty() {
                                size_bytes = digits.parse().ok();
                            }
                        }
                        break;
                    }
                }
                let is_device_local = heap_block.contains("MEMORY_HEAP_DEVICE_LOCAL_BIT");
                if let Some(bytes) = size_bytes {
                    let size_gb = bytes as f64 / 1024.0_f64.powi(3);
                    if is_device_local {
                        device_local_gb = device_local_gb.max(size_gb);
                    } else {
                        host_visible_gb = host_visible_gb.max(size_gb);
                    }
                }
                pos += rel + 12;
            }
        }

        let is_integrated = host_visible_gb > 0.0 && device_local_gb > 0.0;
        gpus.push(GpuInfo {
            name,
            architecture: None,
            driver_version: String::new(),
            temperature: 0.0,
            gpu_percent: 0.0,
            vram: VramInfo {
                percent: 0.0,
                total: if device_local_gb > 0.0 { Some(device_local_gb) } else { None },
                used: Some(0.0),
                r#type: if device_local_gb > 0.0 {
                    Some(if is_integrated { VramType::Shared } else { VramType::Dedicated })
                } else {
                    Some(VramType::Unknown)
                },
                reserved: None,
                gtt: if host_visible_gb > 0.0 { Some(host_visible_gb) } else { None },
            },
            vulkan_heaps: if device_local_gb > 0.0 || host_visible_gb > 0.0 {
                Some(VulkanHeaps {
                    device_local: device_local_gb,
                    host_visible: host_visible_gb,
                })
            } else {
                None
            },
            vendor,
            capabilities: caps_for(vendor),
            gfx_version: None,
            hsa_override_gfx: None,
            op_probes: None,
        });
    }
    gpus
}

/// 取 `key = value` (TS 正则 `/key\s*=\s*(.+)/`)。
fn find_field(s: &str, key: &str) -> Option<String> {
    for line in s.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let v = rest.trim_start();
            if let Some(v) = v.strip_prefix('=') {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// vendorID -> vendor (镜像 TS 两处 vendor 判定)。
fn vendor_from_id(id: u32) -> Vendor {
    match id {
        0x10de => Vendor::Nvidia,
        0x1002 | 0x1022 => Vendor::Amd,
        0x8086 => Vendor::Intel,
        _ => Vendor::Unknown,
    }
}

fn caps_for(vendor: Vendor) -> Capabilities {
    Capabilities {
        webgpu: true,
        vulkan: true,
        cuda: vendor == Vendor::Nvidia,
        rocm: vendor == Vendor::Amd && is_linux(),
        directml: false,
        mps: false,
        openvino: vendor == Vendor::Intel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_mapping() {
        assert_eq!(vendor_from_id(0x10de), Vendor::Nvidia);
        assert_eq!(vendor_from_id(0x1002), Vendor::Amd);
        assert_eq!(vendor_from_id(0x1022), Vendor::Amd);
        assert_eq!(vendor_from_id(0x8086), Vendor::Intel);
        assert_eq!(vendor_from_id(0), Vendor::Unknown);
    }

    #[test]
    fn text_parser_handles_single_device() {
        let sample = r#"...
VkPhysicalDeviceProperties:
deviceName = AMD Radeon Graphics
driverVersion = 123
vendorID = 0x1002
VkPhysicalDeviceLimits:
  maxImageDimension2D = 8192
VkPhysicalDeviceMemoryProperties:
memoryHeaps: count = 2
	memoryHeaps[0]:
		size   = 4294967296 (0x100000000) (4.00 GiB)
		flags:
			None
	memoryHeaps[1]:
		size   = 8589934592 (0x200000000) (8.00 GiB)
		flags: count = 1
			MEMORY_HEAP_DEVICE_LOCAL_BIT
"#;
        let gpus = parse_vulkan_text(sample);
        assert_eq!(gpus.len(), 1);
        let g = &gpus[0];
        assert_eq!(g.name, "AMD Radeon Graphics");
        assert_eq!(g.vendor, Vendor::Amd);
        let heaps = g.vulkan_heaps.as_ref().unwrap();
        assert_eq!(heaps.device_local.round() as i64, 8);
        assert_eq!(heaps.host_visible.round() as i64, 4);
        assert_eq!(g.vram.total.unwrap().round() as i64, 8);
    }
}
