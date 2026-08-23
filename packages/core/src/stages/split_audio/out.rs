use serde::{Deserialize, Serialize};

/// 翻译结果 meta (镜像 TS `packages/core/stages/05_translate/out.ts` TranslateResultMeta)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResultMeta {
    pub src_lang: String,
    pub target_lang: String,
}

/// 意图时序片段 (镜像 TS `SplitAudioTiming`, 继承 TranslateSegment + SubtitleSegment)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioTiming {
    pub seg_idx: u32,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_confidence: Option<f64>,
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// 切分片段 (镜像 TS `SplitAudioSegment` = SplitAudioTiming + split bounds)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioSegment {
    #[serde(flatten)]
    pub timing: SplitAudioTiming,
    /// padSegments 切分音频的起点
    pub split_start_ms: u64,
    /// padSegments 切分音频的终点
    pub split_end_ms: u64,
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
    pub segments: Vec<SplitAudioTiming>,
}
