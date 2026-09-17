//! sf_ocr 阶段 OCR 修正参数 (镜像 TS `packages/core/steps/sf_ocr/fix_args.ts`)。
//!
//! TS 端通过 spread 组合了 `OcrSegmentAdjustArgsSchema` / `BoxAdjustedArgsSchema` /
//! `MergeFramesArgsSchema` / `LlmFixArgsSchema`。`OcrSegmentAdjustArgs` / `BoxAdjustedArgs` /
//! `MergeFramesArgs` 仍内联（subtitle-ocr 的 Rust crate 尚未落地），`LlmFixArgs` 直接复用
//! `llm` crate 的类型，并用 `#[serde(flatten)]` 保持与 TS spread 一致的扁平 JSON 结构。

use std::path::Path;

use llm::LlmFixArgs;
use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::sf_ocr;

/// sf_ocr 阶段 OCR 修正参数。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OcrFixArgs {
    /// 字幕段置信度阈值（0-1）：subtitle-ocr-post filter-segment 用 adjusted_confidence_threshold 过滤，低于此值丢弃；默认 0.45
    #[serde(default = "default_adjusted_confidence_threshold")]
    pub adjusted_confidence_threshold: f64,
    /// 单帧孤立惩罚的参考时间 (ms)，在此时长内无同文帧则视为完全孤立; 默认 1500
    #[serde(default = "default_iso_threshold_ms")]
    pub iso_threshold_ms: f64,
    /// Y 偏移在调整置信度中的权重 (0~1); 默认 0.8
    #[serde(default = "default_adjust_y_weight")]
    pub adjust_y_weight: f64,
    /// 孤立程度在调整置信度中的权重 (0~1); 默认 0.2
    #[serde(default = "default_adjust_iso_weight")]
    pub adjust_iso_weight: f64,
    /// Y 偏移惩罚归一化系数: 偏移量 / (videoHeight × adjustYFactor); 越小越严格; 默认 0.08
    #[serde(default = "default_adjust_y_factor")]
    pub adjust_y_factor: f64,
    /// box调整的置信度阈值: confidence < 此值则进行box调整; 默认 0.5
    #[serde(default = "default_box_adjusted_threshold")]
    pub box_adjusted_threshold: f64,
    /// 是否合并子串
    #[serde(default)]
    pub is_merge_substring: bool,
    /// dedupOverlap 的编辑距离阈值: edit_distance ≤ 此值则合并; 默认 1
    #[serde(default = "default_dedup_edit_distance")]
    pub dedup_edit_distance: u32,
    /// LLM 修正参数 (flatten, 扁平展开为 llmModel / llmApiBase / domainHint / llmFix)
    #[serde(default, flatten)]
    pub llm_fix: LlmFixArgs,
    // 源语言已从参数移除: 统一取 `input.workflow.sourceLang` (> ASR 实测的
    // ctx.asr_language > 默认 zh), 见 ocr_fix 里的解析。
}

impl Default for OcrFixArgs {
    fn default() -> Self {
        Self {
            adjusted_confidence_threshold: default_adjusted_confidence_threshold(),
            iso_threshold_ms: default_iso_threshold_ms(),
            adjust_y_weight: default_adjust_y_weight(),
            adjust_iso_weight: default_adjust_iso_weight(),
            adjust_y_factor: default_adjust_y_factor(),
            box_adjusted_threshold: default_box_adjusted_threshold(),
            is_merge_substring: false,
            dedup_edit_distance: default_dedup_edit_distance(),
            llm_fix: LlmFixArgs::default(),
        }
    }
}

impl OcrFixArgs {
    /// 从输入契约取本步参数：`input.steps.sf_ocr_fix`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.sf_ocr_fix.clone()
    }
}

fn default_adjusted_confidence_threshold() -> f64 {
    0.45
}

fn default_iso_threshold_ms() -> f64 {
    1500.0
}

fn default_adjust_y_weight() -> f64 {
    0.8
}

fn default_adjust_iso_weight() -> f64 {
    0.2
}

fn default_adjust_y_factor() -> f64 {
    0.08
}

fn default_box_adjusted_threshold() -> f64 {
    0.5
}

fn default_dedup_edit_distance() -> u32 {
    1
}

/// sf_ocr_fix 步骤的输入契约：本步要消费的**跨步骤输入**路径。
///
/// 与 `TtsInput` 不同，这里只装真正的输入 `frames_path`——sf_ocr_fix 不回读自己的
/// 任何输出（`srt.json` / 中间产物写完即弃），故无需把输出路径也塞进 struct。
/// 其余路径（`OutDir` / `FilterPath` / `LlmFixPath` / `SrtPath`）仍在 `paths` 模块
/// 类型化、由步骤内部 `::new(video_dir)` 构造，拼法同样只有一处来源。
///
/// 所有路径都是 `{video_dir}/...` 的**约定拼法**，不读 ctx 里落定的事实，故入口是
/// [`from_video_dir`](Self::from_video_dir)（同 `TtsInput::from_video_dir`）。
pub struct SfOcrFixInput {
    /// 来自 sf_ocr 步骤的关键帧 OCR 结果 `sf_ocr/frames.json`。
    pub frames_path: sf_ocr::FramesPath,
}

impl SfOcrFixInput {
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self {
            frames_path: sf_ocr::FramesPath::new(&video_dir),
        }
    }
}
