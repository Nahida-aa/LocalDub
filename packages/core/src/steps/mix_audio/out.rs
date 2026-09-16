//! mix_audio 阶段输出结构 (镜像 TS `packages/core/steps/mix_audio/types.ts`)。

use serde::{Deserialize, Serialize};

use crate::steps::split_audio::out::SplitAudioTiming;
use crate::steps::utils::paths::mix_audio::{self, TimingsPath};
use crate::steps::utils::srt::SubtitleSegments;
use subtitle::SubtitleSegment;

/// 单段对齐时序 (镜像 TS `Timing` = SplitAudioTiming + 对齐字段)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct Timing {
    #[serde(flatten)]
    pub timing: SplitAudioTiming,
    /// 原始时间槽长度 (end - start)
    pub original_duration_ms: u32,
    /// TTS 生成的音频时长
    pub tts_duration_ms: u32,
    /// 去尾静音 + rubberband 拉伸后时长
    pub stretched_duration_ms: u32,
    /// 加速 (拉伸) 比例 (>1.0 = 加速)
    pub stretch_ratio: f64,
    /// drift 累加 (ms)
    pub drift_ms: i32,
    /// 从前面间隙借的时间 (实际比 start 提前)
    pub advance_ms: u32,
    /// 从后面间隙借的时间 (实际比 end 延后)
    pub delay_ms: u32,
    /// 实际开始时间 (考虑了 advance)
    pub actual_start: u32,
    /// 实际结束时间 (考虑了 delay)
    pub actual_end: u32,
}

/// `mix_audio/timings.json` (镜像 TS `MixAudioOutputTimings`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct MixAudioOutputTimings {
    pub segments: Vec<Timing>,
}

impl MixAudioOutputTimings {
    /// 从 [`TimingsPath`] 读出**带类型**的对齐时序。
    ///
    /// 与 [`crate::steps::split_audio::out::SplitAudioTimingResult::read`] 同款：
    /// 解析只在「这里」一处，字段缺失/类型不对直接失败，而不是被静默吞成 0。
    pub fn read(path: &TimingsPath) -> anyhow::Result<Self> {
        let p = path.as_path();
        let raw = std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("读取 mix_audio timings {} 失败: {e}", p.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 mix_audio timings {} 失败: {e}", p.display()))
    }

    /// 写出到 [`TimingsPath`]（pretty json，自动建父目录）。
    ///
    /// 与读对称：序列化和落盘只在「这里」一处，此前 `mix_audio/mod.rs` 各拼一遍。
    pub fn write(&self, path: &mix_audio::TimingsPath) -> anyhow::Result<()> {
        let p = path.as_path();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("序列化 mix_audio timings 失败: {e}"))?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("创建目录 {} 失败: {e}", parent.display()))?;
        }
        std::fs::write(p, json).map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", p.display(), e))?;
        Ok(())
    }
}

impl From<MixAudioOutputTimings> for SubtitleSegments {
    /// 对齐时序 → 规范化字幕：烧进视频的字幕要跟**配音**对齐，故时间轴取
    /// `actual_start` / `actual_end`（mix_audio 拉伸、借间隙后的实际位置），
    /// 而不是 `Timing.timing` 里的原始时间槽。
    ///
    /// 文本取译文 `dst`，`dst` 为空时回退原文 `text`。
    fn from(v: MixAudioOutputTimings) -> Self {
        v.segments
            .into_iter()
            .map(|t| {
                let base = &t.timing;
                let dst = base.dst.trim();
                SubtitleSegment {
                    text: if dst.is_empty() {
                        base.text.trim().to_string()
                    } else {
                        dst.to_string()
                    },
                    start_ms: t.actual_start,
                    end_ms: t.actual_end,
                }
            })
            .collect::<Vec<_>>()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::translate::out::TranslateSegment;

    fn timing(
        text: &str,
        dst: &str,
        start_ms: u32,
        end_ms: u32,
        a_start: u32,
        a_end: u32,
    ) -> Timing {
        Timing {
            timing: TranslateSegment {
                idx: 1,
                text: text.into(),
                dst: dst.into(),
                start_ms,
                end_ms,
                ..Default::default()
            },
            actual_start: a_start,
            actual_end: a_end,
            ..Default::default()
        }
    }

    /// 时间轴必须取 actual_*：字幕要跟**配音**对齐，而不是原始时间槽。
    #[test]
    fn into_subtitle_segments_uses_actual_times() {
        let v = MixAudioOutputTimings {
            segments: vec![timing("原文", "译文", 0, 1000, 120, 980)],
        };
        let segs: SubtitleSegments = v.into();
        assert_eq!(segs.0.len(), 1);
        assert_eq!(segs.0[0].start_ms, 120);
        assert_eq!(segs.0[0].end_ms, 980);
    }

    /// 文本取译文 dst；dst 为空回退原文 text。
    #[test]
    fn into_subtitle_segments_prefers_dst_then_text() {
        let v = MixAudioOutputTimings {
            segments: vec![
                timing("原文", "译文", 0, 1000, 0, 1000),
                timing("回退原文", "   ", 1000, 2000, 1000, 2000),
            ],
        };
        let segs: SubtitleSegments = v.into();
        assert_eq!(segs.0[0].text, "译文");
        assert_eq!(segs.0[1].text, "回退原文");
    }
}
