use serde::{Deserialize, Serialize};

/// 切分项 (镜像 TS `packages/core/stages/06_split_audio/types.ts` SplitAudioItem)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioItem {
    pub seg_idx: u32,
    pub src: String,
    pub dst: String,
    pub src_lang: String,
    pub dst_lang: String,
    /// padSegments 切分音频的起点
    pub start: u64,
    /// padSegments 切分音频的终点
    pub end: u64,
    pub speaker: String,
}

/// 视频意图时间 (镜像 TS SplitAudioTiming, start/end 为未 padSegments 的意图时序)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioTiming {
    pub seg_idx: u32,
    pub src: String,
    pub dst: String,
    pub src_lang: String,
    pub dst_lang: String,
    /// 视频意图起点
    pub start: u64,
    /// 视频意图终点
    pub end: u64,
    pub speaker: String,
}

/// `split_audio/split_audio.json` (padSegments 后时序)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioFile {
    pub translation: Vec<SplitAudioItem>,
}

/// `split_audio/timings.json` (意图时序)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SplitAudioTimingFile {
    pub translation: Vec<SplitAudioTiming>,
}
