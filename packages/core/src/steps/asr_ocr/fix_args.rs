//! asr_ocr 阶段 OCR 修正参数 (镜像 TS `packages/core/steps/04_asr_ocr/fix_args.ts`)。
//!
//! TS 端 `AsrOcrFixArgsSchema` 在 `OcrFixArgsSchema` 基础上 spread 并追加 `is_resample`。
//! 这里复用 `sf_ocr::fix_args::OcrFixArgs`，用 `#[serde(flatten)]` 保持扁平 JSON 结构，
//! 再追加 `is_resample`。

use crate::steps::sf_ocr::fix_args::OcrFixArgs;
use crate::steps::utils::paths::{asr_ocr, asr_ocr_pre};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// asr_ocr 阶段 OCR 修正参数。
///
/// 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
/// `impl Default` 以保证默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrOcrFixArgs {
    /// OCR 修正参数 (flatten, 继承 OcrFixArgs 的全部扁平字段)
    #[serde(default, flatten)]
    pub ocr_fix: OcrFixArgs,
    /// 是否重采样
    #[serde(default)]
    pub is_resample: bool,
}

impl Default for AsrOcrFixArgs {
    fn default() -> Self {
        Self {
            ocr_fix: OcrFixArgs::default(),
            is_resample: false,
        }
    }
}

impl AsrOcrFixArgs {
    /// 从输入契约取本步参数：`input.steps.asr_ocr_fix`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.asr_ocr_fix.clone()
    }
}

/// asr_ocr_fix 步骤的跨步骤输入清单。
///
/// 只收两条上游产物（均为「约定拼法」的 `{video_dir}/...`，故用
/// [`from_video_dir`](Self::from_video_dir) 入口，与 `AsrFixInput` / `SfOcrFixInput`
/// 同款）：
///
/// - `asr_ocr_pre/asr_split.json`（`asr_ocr_pre` 步骤，[`asr_ocr_pre::AsrSplitPath`]）——
///   原始 ASR 段数已记入其 `meta.original_segments_count`，故本步**不再**回读
///   `asr/asr.json`（那是 `asr_ocr_pre` 的输入，不是本步该直接碰的）
/// - `asr_ocr/frames.json`（`asr_ocr` 步骤，[`asr_ocr::FramesPath`]）
///
/// 自身产物 `asr_ocr_fix/srt.json` 不在此列——那是输出，非输入。
pub struct AsrOcrFixInput {
    /// 来自 asr_ocr_pre 步骤的 ASR 切分结果 `asr_ocr_pre/asr_split.json`。
    pub asr_ocr_pre_asr_split_path: asr_ocr_pre::AsrSplitPath,
    /// 来自 asr_ocr 步骤的逐帧 OCR 结果 `asr_ocr/frames.json`。
    pub ocr_frames_path: asr_ocr::FramesPath,
}

impl AsrOcrFixInput {
    /// 按约定拼法构造：`{video_dir}/asr_ocr_pre/asr_split.json` 等两条。
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self {
            asr_ocr_pre_asr_split_path: asr_ocr_pre::AsrSplitPath::new(&video_dir),
            ocr_frames_path: asr_ocr::FramesPath::new(&video_dir),
        }
    }
}
