//! tts 阶段输出结构 (镜像 TS `packages/core/stages/07_tts/out.ts`)。

use serde::{Deserialize, Serialize};

use crate::stages::split_audio::out::SplitAudioTiming;

/// 单条 TTS 段 (镜像 TS `TtsSegment` = SplitAudioTiming + tts 字段)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TtsSegment {
    #[serde(flatten)]
    pub timing: SplitAudioTiming,
    /// split_audio end_ms (原始槽位终点, 参考)
    pub slot_end_ms: u64,
    /// TTS 生成音频时长
    pub tts_duration_ms: u64,
    /// 状态: success / skipped / error / empty
    pub status: String,
}

/// `tts/tts.json` (镜像 TS `TtsFile`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TtsFile {
    pub segments: Vec<TtsSegment>,
}
