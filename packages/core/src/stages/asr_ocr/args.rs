//! asr_ocr 阶段参数 (镜像 TS `packages/core/stages/04_asr_ocr/args.ts`)。
//!
//! TS 端 `AsrOcrArgsSchema` 复用 `sf_ocr/args` 的 `ocrRuntimeSchema`，字段集与 `SfOcrArgs`
//! 完全一致 (runtime / device / text_confidence_threshold / subtitleOnly / cleanupFrames)。
//! 这里复用 `sf_ocr::args::SfOcrArgs`，用 `#[serde(flatten)]` 保持扁平 JSON 结构，
//! 同时保留 asr_ocr 独立的类型身份 (便于将来与 sf_ocr 分叉)。

use crate::stages::sf_ocr::args::SfOcrArgs;
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段参数。
///
/// 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
/// `impl Default` 以保证默认值一致 (与 `input::stages::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
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
