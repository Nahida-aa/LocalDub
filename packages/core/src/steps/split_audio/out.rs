use serde::{Deserialize, Serialize};

use crate::steps::translate::out::TranslateSegment;

/// 翻译结果 meta (镜像 TS `packages/core/steps/05_translate/out.ts` TranslateResultMeta)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResultMeta {
    pub src_lang: String,
    pub target_lang: String,
}

/// 意图时序片段 (镜像 TS `SplitAudioTiming`, 继承 TranslateSegment + SubtitleSegment)
// #[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub type SplitAudioTiming = TranslateSegment;

/// 切分片段 (镜像 TS `SplitAudioSegment` = SplitAudioTiming + split bounds)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioSegment {
    #[serde(flatten)]
    pub timing: TranslateSegment,
    /// padSegments 切分音频的起点
    pub split_start_ms: u32,
    /// padSegments 切分音频的终点
    pub split_end_ms: u32,
}

/// `split_audio/split_audio.json` (padSegments 后时序 + meta)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioResult {
    pub segments: Vec<SplitAudioSegment>,
    pub meta: TranslateResultMeta,
}

/// `split_audio/timings.json` (意图时序)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioTimingResult {
    pub segments: Vec<TranslateSegment>,
}
