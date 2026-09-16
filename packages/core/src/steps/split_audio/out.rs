use serde::{Deserialize, Serialize};

use crate::steps::translate::out::TranslateSegment;
use crate::steps::utils::paths::split_audio::TimingsPath;

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

impl SplitAudioTimingResult {
    /// 从 [`TimingsPath`] 读出**带类型**的意图时序。
    ///
    /// 与 [`crate::steps::translate::out::TranslateResult::read`] 同款：
    /// 解析只在「这里」一处，字段缺失/类型不对会直接失败，而不是被静默吞成 0。
    /// `MixAudioInput::from_state` 直接用它拿类型化时序。
    pub fn read(path: &TimingsPath) -> anyhow::Result<Self> {
        let p = path.as_path();
        let raw = std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("读取 split_audio timings {} 失败: {e}", p.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 split_audio timings {} 失败: {e}", p.display()))
    }
}
