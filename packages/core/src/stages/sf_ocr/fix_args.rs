//! sf_ocr 阶段 OCR 修正参数 (镜像 TS `packages/core/stages/sf_ocr/fix_args.ts`)。
//!
//! TS 端通过 spread 组合了 `OcrSegmentAdjustArgsSchema` / `BoxAdjustedArgsSchema` /
//! `MergeFramesArgsSchema` / `LlmFixArgsSchema`，这里内联同一组字段
//! (subtitle-ocr / llm 的 Rust crate 尚未落地，不宜跨 crate 依赖)。

use serde::{Deserialize, Serialize};

/// sf_ocr 阶段 OCR 修正参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OcrFixArgs {
    /// 字幕段置信度阈值（0-1）：ocr-post filter-segment 用 adjusted_confidence_threshold 过滤，低于此值丢弃；默认 0.45
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
    /// LLM 模型名
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    /// LLM API 地址
    #[serde(default = "default_llm_api_base")]
    pub llm_api_base: String,
    /// 领域提示, 帮助 LLM 理解上下文，例如"仙侠题材，角色：叶白、慧天、夜白"
    #[serde(default)]
    pub domain_hint: Option<String>,
    /// 是否启用 LLM 修正
    #[serde(default)]
    pub llm_fix: bool,
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

fn default_llm_model() -> String {
    "gemma4:31b-cloud".to_string()
}

fn default_llm_api_base() -> String {
    "http://localhost:11434/v1".to_string()
}
