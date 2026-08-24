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

/// 增量进度文件 `translate/translation.{lang}.partial.json`。
///
/// 每完成一个 batch 即落盘, 记录已翻译的句子 (含缺失标注) 与已完成 batch 索引,
/// 用于: (1) 阶段内续跑时跳过已完成 batch; (2) 把"不等的结果" (缺失句) 也存下来方便分析。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslatePartialResult {
    /// 与最终 `translation.{lang}.json` 同结构的段数组 (缺失句 dst 为空 + missing=true)
    pub segments: Vec<TranslatePartialSegment>,
    /// 已完成 (全部句翻译成功) 的 batch 索引集合
    #[serde(default)]
    pub completed_batches: Vec<usize>,
    pub meta: TranslateResultMeta,
}

/// partial 单段: 在 [`TranslateSegment`] 基础上加 batch 索引与缺失标记。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslatePartialSegment {
    pub text: String,
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_lang: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    /// 所属 batch 索引 (0-based)
    #[serde(default)]
    pub batch_index: usize,
    /// 该句是否缺失 (翻译失败/为空)
    #[serde(default)]
    pub missing: bool,
}
