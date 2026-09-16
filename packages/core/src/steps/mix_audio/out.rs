//! mix_audio 阶段输出结构 (镜像 TS `packages/core/steps/mix_audio/types.ts`)。

use serde::{Deserialize, Serialize};

use crate::steps::split_audio::out::SplitAudioTiming;
use crate::steps::utils::paths::mix_audio::TimingsPath;

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

/// `mix_audio/timings.json` (镜像 TS `MixAudioOuputTimings`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct MixAudioOuputTimings {
    pub segments: Vec<Timing>,
}

impl MixAudioOuputTimings {
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
    pub fn write(&self, path: &TimingsPath) -> anyhow::Result<()> {
        let p = path.as_path();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("序列化 mix_audio timings 失败: {e}"))?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("创建目录 {} 失败: {e}", parent.display()))?;
        }
        std::fs::write(p, json)
            .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", p.display(), e))?;
        Ok(())
    }
}
