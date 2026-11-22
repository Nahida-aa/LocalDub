//! asr_ocr 阶段参数 (镜像 TS `packages/core/steps/04_asr_ocr/args.ts`)。
//!
//! TS 端 `AsrOcrArgsSchema` 复用 `sf_ocr/args` 的 `ocrRuntimeSchema`，字段集与 `SfOcrArgs`
//! 完全一致 (runtime / device / text_confidence_threshold / subtitleOnly / cleanupFrames)。
//! 这里复用 `sf_ocr::args::SfOcrArgs`，用 `#[serde(flatten)]` 保持扁平 JSON 结构，
//! 同时保留 asr_ocr 独立的类型身份 (便于将来与 sf_ocr 分叉)。

use crate::steps::sf_ocr::args::SfOcrArgs;
use crate::steps::utils::paths::asr_ocr_pre;
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段参数。
///
/// 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
/// `impl Default` 以保证默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct AsrOcrArgs {
    /// OCR 参数 (flatten, 继承 SfOcrArgs 的全部扁平字段)
    #[serde(default, flatten)]
    pub ocr: SfOcrArgs,
}

impl Default for AsrOcrArgs {
    fn default() -> Self {
        Self {
            ocr: SfOcrArgs::default(),
        }
    }
}

impl AsrOcrArgs {
    /// 从输入契约取本步参数：`input.steps.asr_ocr`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.asr_ocr.clone()
    }
}

/// Phase 1 事实面：`step_asr_ocr` 的显式依赖清单（由装配层算好后注入）。
///
/// 只装一条上游产物：`asr_ocr_pre` 步骤产出的帧目录
/// [`asr_ocr_pre::FramesDir`]——交给 `subtitle-ocr` 二进制做 OCR 的输入（`--dir`）。
/// 自身产物 `asr_ocr/frames.json` 不在此列——那是输出，非输入。
///
/// 意图面（`video_dir` / `args`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct AsrOcrDeps {
    /// asr_ocr_pre 步骤产出的帧目录。
    pub frames_dir: asr_ocr_pre::FramesDir,
}

/// `step_asr_ocr` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct AsrOcrOutput {
    /// OCR 结果 `asr_ocr/frames.json`。
    pub frames_path: std::path::PathBuf,
}

impl AsrOcrDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        Self {
            frames_dir: asr_ocr_pre::FramesDir::new(input.video_dir()),
        }
    }
}
