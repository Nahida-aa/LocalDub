//! tts 阶段输出结构 (镜像 TS `packages/core/steps/07_tts/out.ts`)。

use serde::{Deserialize, Serialize};

use crate::steps::split_audio::out::SplitAudioTiming;
use crate::steps::utils::paths::tts::TtsPath;

/// 单条 TTS 段 (镜像 TS `TtsSegment` = SplitAudioTiming + tts 字段)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TtsSegment {
    #[serde(flatten)]
    pub timing: SplitAudioTiming,
    /// split_audio end_ms (原始槽位终点, 参考)
    pub slot_end_ms: u32,
    /// TTS 生成音频时长
    pub tts_duration_ms: u32,
    /// 状态: success / skipped / error / empty
    pub status: String,
}

/// `tts/tts.json` (镜像 TS `TtsSegmentsResult`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TtsSegmentsResult {
    pub segments: Vec<TtsSegment>,
}

impl TtsSegmentsResult {
    /// 从 [`TtsPath`] 读出**带类型**的 tts 结果。
    ///
    /// 与 [`crate::steps::split_audio::out::SplitAudioTimingResult::read`] 同款：
    /// 解析只在「这里」一处，字段缺失/类型不对直接失败，而不是被静默吞成默认值。
    pub fn read(path: &TtsPath) -> anyhow::Result<Self> {
        let p = path.as_path();
        let raw = std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("读取 tts 结果 {} 失败: {e}", p.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 tts 结果 {} 失败: {e}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 坏 JSON 必须**报错**而不是反序列化成默认值——调用方靠 Err 区分
    /// 「读不出来」与「本来就没有旧结果」。
    #[test]
    fn read_rejects_invalid_json() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(format!("ld_tts_read_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tts.json");
        std::fs::write(&p, "{ not json").unwrap();

        let err = TtsSegmentsResult::read(&TtsPath::new(&dir)).expect_err("坏 JSON 必须报错");
        assert!(err.to_string().contains("tts.json"), "实际: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
