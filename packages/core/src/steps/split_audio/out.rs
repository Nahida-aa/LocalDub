use serde::{Deserialize, Serialize};

use crate::steps::translate::out::TranslateSegment;
use crate::steps::utils::paths::split_audio;
use crate::steps::utils::srt::SubtitleSegments;
use subtitle::SubtitleSegment;

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
    /// 从 [`split_audio::TimingsPath`] 读出**带类型**的意图时序。
    ///
    /// 与 [`crate::steps::translate::out::TranslateResult::read`] 同款：
    /// 解析只在「这里」一处，字段缺失/类型不对会直接失败，而不是被静默吞成 0。
    /// `MixAudioInput::from_state` 直接用它拿类型化时序。
    pub fn read(path: &split_audio::TimingsPath) -> anyhow::Result<Self> {
        let p = path.as_path();
        let raw = std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("读取 split_audio timings {} 失败: {e}", p.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 split_audio timings {} 失败: {e}", p.display()))
    }
}

impl From<SplitAudioTimingResult> for SubtitleSegments {
    /// 意图时序 → 规范化字幕：直接用段自身的 `start_ms` / `end_ms`
    /// （意图时序没有 `actual_*`，这与 [`crate::steps::mix_audio::out::MixAudioOutputTimings`]
    /// 取 `actual_start` / `actual_end` 不同——后者是配音**实际**落点）。
    ///
    /// 文本固定取译文 `dst`，`dst` 为空回退原文 `text`，与
    /// [`crate::steps::mix_audio::out::MixAudioOutputTimings`] 的转换同款。
    ///
    /// 固定取 `dst`（空则回退 `text`）**已经覆盖不翻译模式**：translate 在
    /// `enabled=false` 时走直通，把原文写进 `dst`，所以此时 `dst` 就是原文——
    /// 不需要 `write_srt(.., use_source = true)` 那样的第二个入口。
    fn from(v: SplitAudioTimingResult) -> Self {
        v.segments
            .into_iter()
            .map(|s| {
                let dst = s.dst.trim();
                SubtitleSegment {
                    text: if dst.is_empty() {
                        s.text.trim().to_string()
                    } else {
                        dst.to_string()
                    },
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                }
            })
            .collect::<Vec<_>>()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, dst: &str, start_ms: u32, end_ms: u32) -> TranslateSegment {
        TranslateSegment {
            idx: 1,
            text: text.into(),
            dst: dst.into(),
            start_ms,
            end_ms,
            ..Default::default()
        }
    }

    /// 意图时序没有 actual_*，时间轴直接用段自身的 start_ms/end_ms。
    #[test]
    fn into_subtitle_segments_uses_own_times() {
        let v = SplitAudioTimingResult {
            segments: vec![seg("原文", "译文", 120, 980)],
        };
        let segs: SubtitleSegments = v.into();
        assert_eq!(segs.0.len(), 1);
        assert_eq!(segs.0[0].start_ms, 120);
        assert_eq!(segs.0[0].end_ms, 980);
    }

    /// 文本取译文 dst；dst 为空回退原文 text（与 WriteSrt use_source=false 同款）。
    #[test]
    fn into_subtitle_segments_prefers_dst_then_text() {
        let v = SplitAudioTimingResult {
            segments: vec![seg("原文", "译文", 0, 1000), seg("回退原文", "  ", 1000, 2000)],
        };
        let segs: SubtitleSegments = v.into();
        assert_eq!(segs.0[0].text, "译文");
        assert_eq!(segs.0[1].text, "回退原文");
    }
}
