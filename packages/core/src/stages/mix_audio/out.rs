//! mix_audio 阶段输出结构 (镜像 TS `packages/core/stages/mix_audio/types.ts`)。

use serde::{Deserialize, Serialize};

use crate::stages::split_audio::out::SplitAudioTiming;

/// 单段对齐时序 (镜像 TS `Timing` = SplitAudioTiming + 对齐字段)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct Timing {
    #[serde(flatten)]
    pub timing: SplitAudioTiming,
    /// 原始时间槽长度 (end - start)
    pub original_duration_ms: u64,
    /// TTS 生成的音频时长
    pub tts_duration_ms: u64,
    /// 去尾静音 + rubberband 拉伸后时长
    pub stretched_duration_ms: u64,
    /// 加速 (拉伸) 比例 (>1.0 = 加速)
    pub stretch_ratio: f64,
    /// drift 累加 (ms)
    pub drift_ms: i64,
    /// 从前面间隙借的时间 (实际比 start 提前)
    pub advance_ms: u64,
    /// 从后面间隙借的时间 (实际比 end 延后)
    pub delay_ms: u64,
    /// 实际开始时间 (考虑了 advance)
    pub actual_start: u64,
    /// 实际结束时间 (考虑了 delay)
    pub actual_end: u64,
}

/// `mix_audio/timings.json` (镜像 TS `TimingsFile`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TimingsFile {
    pub segments: Vec<Timing>,
}
