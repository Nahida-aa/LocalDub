//! sf_ocr 阶段参数 (镜像 TS `packages/core/stages/sf_ocr/args.ts`)。

use serde::{Deserialize, Serialize};

/// OCR 推理运行时。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum OcrRuntime {
    /// ort-rust (opencv)
    #[default]
    #[serde(rename = "ort-rust")]
    OrtRust,
}

/// OCR 运行设备。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum OcrDevice {
    /// cpu
    #[default]
    Cpu,
    /// cuda (NVIDIA)
    Cuda,
    /// directml (Windows)
    Directml,
    /// coreml (macOS)
    Coreml,
    /// rocm (AMD)
    Rocm,
    /// mps (Apple Silicon)
    Mps,
}

/// sf_ocr 阶段参数。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::stages::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SfOcrArgs {
    /// OCR 推理运行时: ort-rust (opencv)
    #[serde(default = "default_runtime")]
    pub runtime: OcrRuntime,
    /// OCR 运行设备: cpu, cuda, directml, coreml, rocm, mps
    #[serde(default = "default_device")]
    pub device: OcrDevice,
    /// OCR 识别置信度阈值, 默认 0.45
    #[serde(default = "default_text_confidence_threshold")]
    pub text_confidence_threshold: f64,
    /// 只识别字幕区域 (Y轴裁剪); 默认 true
    #[serde(default = "default_true")]
    pub subtitle_only: bool,
    /// 步骤完成后是否删除抽出的帧图片; 默认 false (保留)
    #[serde(default)]
    pub cleanup_frames: bool,
}

impl Default for SfOcrArgs {
    fn default() -> Self {
        Self {
            runtime: default_runtime(),
            device: default_device(),
            text_confidence_threshold: default_text_confidence_threshold(),
            subtitle_only: default_true(),
            cleanup_frames: false,
        }
    }
}

fn default_runtime() -> OcrRuntime {
    OcrRuntime::OrtRust
}

fn default_device() -> OcrDevice {
    OcrDevice::Cpu
}

fn default_text_confidence_threshold() -> f64 {
    0.45
}

fn default_true() -> bool {
    true
}
