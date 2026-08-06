use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum Vendor {
    #[serde(rename = "amd")]
    Amd,
    #[serde(rename = "nvidia")]
    Nvidia,
    #[serde(rename = "intel")]
    Intel,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum VramType {
    #[serde(rename = "dedicated")]
    Dedicated,
    #[serde(rename = "shared")]
    Shared,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VramInfo {
    pub percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<VramType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gtt: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub webgpu: bool,
    pub vulkan: bool,
    pub cuda: bool,
    pub rocm: bool,
    pub directml: bool,
    pub mps: bool,
    pub openvino: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VulkanHeaps {
    pub device_local: f64,
    pub host_visible: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]

pub enum ProbeResult {
    // #[serde(rename = "ok")]
    Ok,
    // #[serde(rename = "fail")]
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OpProbes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torch_conv1d: Option<ProbeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Vendor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    pub driver_version: String,
    pub temperature: f64,
    pub gpu_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gfx_version: Option<String>,
    pub vram: VramInfo,
    pub capabilities: Capabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hsa_override_gfx: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vulkan_heaps: Option<VulkanHeaps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_probes: Option<OpProbes>,
}
