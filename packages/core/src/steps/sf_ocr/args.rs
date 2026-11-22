//! sf_ocr 阶段参数 (镜像 TS `packages/core/steps/sf_ocr/args.ts`)。

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::sf_ocr_pre;

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
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
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

impl SfOcrArgs {
    /// 从输入契约取本步参数：`input.steps.sf_ocr`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.sf_ocr.clone()
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

/// Phase 1 事实面：`step_sf_ocr` 的显式依赖清单（由装配层算好后注入）。
///
/// 只装依赖——`sf_ocr_pre` 产出的关键帧目录。本步产出 `frames.json`
/// （`sf_ocr::FramesPath`）属于「产出」而非「依赖」，不在此清单内。
///
/// 意图面（`video_dir` / `args`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct SfOcrDeps {
    /// sf_ocr_pre 步骤产出的关键帧目录。
    pub keyframes_dir: sf_ocr_pre::KeyframesDir,
}

/// `step_sf_ocr` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct SfOcrOutput {
    /// OCR 结果 `sf_ocr/frames.json`。
    pub frames_path: std::path::PathBuf,
}

impl SfOcrDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        Self {
            keyframes_dir: sf_ocr_pre::KeyframesDir::new(input.video_dir()),
        }
    }
}
