//! asr_ocr 阶段 OCR 修正参数 (镜像 TS `packages/core/steps/04_asr_ocr/fix_args.ts`)。
//!
//! TS 端 `AsrOcrFixArgsSchema` 在 `OcrFixArgsSchema` 基础上 spread 并追加 `is_resample`。
//! 这里复用 `sf_ocr::fix_args::OcrFixArgs`，用 `#[serde(flatten)]` 保持扁平 JSON 结构，
//! 再追加 `is_resample`。

use crate::steps::sf_ocr::fix_args::OcrFixArgs;
use crate::steps::utils::paths::{asr_ocr, asr_ocr_pre};
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段 OCR 修正参数。
///
/// 注意: 父结构用 `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写
/// `impl Default` 以保证默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
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

/// Phase 1 事实面：`step_asr_ocr_fix` 的显式依赖清单（由装配层算好后注入）。
///
/// 装本步消费的三条上游产物 / 事实（各字段注释标明生产者）：
/// - `asr_ocr_pre` 产出的 `asr_split.json`——原始 ASR 段数已记入其
///   `meta.original_segments_count`，故本步不回读 `asr/asr.json`；
/// - `asr_ocr` 产出的 `frames.json`（逐帧 OCR 结果）；
/// - `source_lang`：装配层按三级回退算好的源语言，LLM 提示词用。
///
/// 意图面（`video_dir` / `args` / `max_advance_ms`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct AsrOcrFixDeps {
    /// asr_ocr_pre 步骤产出的 ASR 切分结果。 asr_ocr_pre/asr_split.json
    pub asr_ocr_pre_asr_split_path: asr_ocr_pre::AsrSplitPath,
    /// asr_ocr 步骤产出的逐帧 OCR 结果。 asr_ocr/frames.json
    pub ocr_frames_path: asr_ocr::FramesPath,
    /// 装配层算好的源语言（LLM 提示词用）。
    pub source_lang: crate::r#const::lang::LangStr,
}

/// `step_asr_ocr_fix` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
///
/// 所有写出的 json 全列（含两个条件产物）：中间态五个 + `asr_ocr_merged` /
/// `fused` / 规范化字幕 `srt.json` 是无条件写出的；`ocr_frames.json` 与
/// `resampled_frames/` 仅 is_resample 且真的有新候选帧时产出，
/// `asr_ocr_fused_llm_fix.json` 仅 llmFix 调用成功时写。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct AsrOcrFixOutput {
    pub frames_box_adjust_path: std::path::PathBuf,
    pub frames_box_filter_path: std::path::PathBuf,
    pub frames_merged_path: std::path::PathBuf,
    pub segment_adjust_path: std::path::PathBuf,
    pub segment_filter_path: std::path::PathBuf,
    /// is_resample 且真的有新候选帧时产出的重采样合并帧。
    pub ocr_frames_path: Option<std::path::PathBuf>,
    /// 同上：重采样抽出的帧目录。
    pub resampled_frames_dir: Option<std::path::PathBuf>,
    pub asr_ocr_merged_path: std::path::PathBuf,
    pub asr_ocr_fused_path: std::path::PathBuf,
    /// llmFix 调用成功时产出。
    pub asr_ocr_fused_llm_fix_path: Option<std::path::PathBuf>,
    /// 规范化字幕（下游唯一契约）。
    pub srt_path: std::path::PathBuf,
}

impl AsrOcrFixDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        let video_dir = input.video_dir();
        let asr_language = crate::steps::asr::out::asr_language(video_dir);
        let source_lang = crate::state::lang::resolve_source_language(
            asr_language.as_ref(),
            input.workflow.source_lang.as_ref(),
        );
        Self {
            asr_ocr_pre_asr_split_path: asr_ocr_pre::AsrSplitPath::new(video_dir),
            ocr_frames_path: asr_ocr::FramesPath::new(video_dir),
            source_lang,
        }
    }
}
