//! asr_ocr 阶段 OCR 修正参数 (镜像 TS `packages/core/stages/04_asr_ocr/fix_args.ts`)。
//!
//! TS 端 `AsrOcrFixArgsSchema` 在 `OcrFixArgsSchema` 基础上 spread 并追加 `is_resample`。
//! 这里复用 `sf_ocr::fix_args::OcrFixArgs`，用 `#[serde(flatten)]` 保持扁平 JSON 结构，
//! 再追加 `is_resample`。

use crate::stages::sf_ocr::fix_args::OcrFixArgs;
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段 OCR 修正参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrOcrFixArgs {
    /// OCR 修正参数 (flatten, 继承 OcrFixArgs 的全部扁平字段)
    #[serde(default, flatten)]
    pub ocr_fix: OcrFixArgs,
    /// 是否重采样
    #[serde(default)]
    pub is_resample: bool,
}
