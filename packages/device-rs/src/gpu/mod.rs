mod types;
pub use types::*;

/// GPU 采集入口 (镜像 TS `gpu/gpu.ts::getGpuInfo`)。
mod collect;
/// vulkaninfo 解析 (json + 文本回退)。
pub(crate) mod vulkan;
pub use collect::get_gpu_info;
pub(crate) use vulkan::try_vulkan_info;

/// 当前平台是否为 linux (镜像 TS `process.platform === 'linux'`)。
pub(crate) fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// 去重归一: 去掉 Vulkan 后缀差异 `(RADV PHOENIX)` 等 (镜像 TS `normName`)。
///
/// 正则: `/\s*\([A-Z]+ .*\)$/` —— 小写/混合后缀如 `(gfx1100)` 不匹配, 保留;
/// 仅匹配 `(` 后紧跟大写字母 + 空格的后缀。
pub(crate) fn norm_name(n: &str) -> String {
    let trimmed = n.trim();
    // 找最后一个 ` (XXXX ...)` 且括号前是空白、括号内首词全大写。
    if let Some(open) = trimmed.rfind(" (") {
        let after = trimmed[open + 2..].trim_start();
        let first_word = after.split_whitespace().next().unwrap_or("");
        let is_upper_start = !first_word.is_empty()
            && first_word.chars().next().unwrap().is_ascii_uppercase()
            && after.ends_with(')');
        if is_upper_start {
            return trimmed[..open + 1].trim_end().to_string();
        }
    }
    trimmed.to_string()
}
