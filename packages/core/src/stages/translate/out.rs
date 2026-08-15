//! translate 阶段输出结构 (镜像 TS `packages/core/stages/05_translate/out.ts`)。

use serde::{Deserialize, Serialize};

/// 单条翻译段 (镜像 TS `TranslateSegment`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateSegment {
    /// 原文 (识别文本)
    pub text: String,
    /// 译文 (dubbed / subtitled)
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_lang: Option<String>,
    /// 段起点 (ms)
    pub start_ms: u64,
    /// 段终点 (ms)
    pub end_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// `translate/translation.{lang}.json` 结构 (镜像 TS `TranslateResult`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResult {
    pub segments: Vec<TranslateSegment>,
    pub meta: TranslateResultMeta,
}

/// translate 结果 meta (镜像 TS `TranslateResultMeta`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResultMeta {
    pub src_lang: String,
    pub target_lang: String,
}
