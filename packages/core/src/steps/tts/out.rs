//! tts 阶段输出结构 (镜像 TS `packages/core/steps/07_tts/out.ts`)。

use std::collections::HashMap;

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

/// `step_tts` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
///
/// 与 [`TtsSegmentsResult`] 的分工：后者只是 `tts.json` 的**内容类型**，本类型
/// 是本步骤**产出了哪些文件**的清单——tts.json + 逐段配音 wav 目录 +（可选）
/// 参考音翻倍拼接目录。
///
/// `ref_doubled_dir` 用 `Option` 表达「本次是否真的产出」：只有 `ref_audio_x2`
/// 打开时才创建 `doubled/`。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct TtsOutput {
    /// 逐段结果 `tts/tts.json`。
    pub result_path: std::path::PathBuf,
    /// 逐段配音 wav 目录 `tts/wavs/`。
    pub wavs_dir: std::path::PathBuf,
    /// 参考音翻倍拼接目录 `tts/doubled/`——仅 `ref_audio_x2` 产出。
    pub ref_doubled_dir: Option<std::path::PathBuf>,
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

    /// 读取 tts.json 并构建按 `idx` 索引的已有段 map, 供 regen 时复用。
    ///
    /// 与 [`Self::read`] 不同, 这里对**缺失/损坏**做 graceful 退化而非报错:
    /// - 文件不存在 → 空 map (语义: 无旧结果, 下方 regenIndices 外的段会重新生成)
    /// - 文件存在但读/解析失败 → 空 map + 一条 warn (区别于「本就不存在」, 不能静默)
    pub fn read_existing_segments(path: &TtsPath) -> HashMap<u32, TtsSegment> {
        let p = path.as_path();
        if !p.exists() {
            return HashMap::new();
        }
        match Self::read(path) {
            Ok(f) => f.segments.into_iter().map(|s| (s.timing.idx, s)).collect(),
            Err(e) => {
                tracing::warn!(target: "tts",
                    "读取 {} 失败, 按无旧结果处理 (regenIndices 外的段将重新生成): {e}",
                    p.display()
                );
                HashMap::new()
            }
        }
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
