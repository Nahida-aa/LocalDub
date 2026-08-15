//! 音频范围切块 (镜像 TS `packages/core/stages/06_split_audio/util.ts` cutAudioRange)。

use crate::stages::utils::ffmpeg;

/// 从源音频流拷贝 `[startMs, endMs)` 范围到 `out_path`, 不重编码 (`-c copy`, 快)。
/// 参数取毫秒, 内部转秒交给 ffmpeg。
pub fn cut_audio_range(
    source: &str,
    start_ms: u64,
    end_ms: u64,
    out_path: &str,
) -> anyhow::Result<()> {
    ffmpeg(&[
        "-i".to_string(),
        source.to_string(),
        "-ss".to_string(),
        format!("{}", start_ms as f64 / 1000.0),
        "-to".to_string(),
        format!("{}", end_ms as f64 / 1000.0),
        "-c".to_string(),
        "copy".to_string(),
        out_path.to_string(),
    ])
}
