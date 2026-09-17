//! separate_after 步骤的跨步骤依赖清单(代码即 DAG 视角)。

use crate::steps::utils::paths::separate;
use std::path::Path;

/// separate_after 步骤的跨步骤依赖清单。
///
/// 与 `AsrOcrPreInput` 等兄弟 `*Input` 同款定位: 只声明**被消费的跨步骤产物**,
/// 自身输出(stems / bgm / mixed / gated)、`*Args::from_input` 另行获取。
///
/// separate_after 直接对 `separate` 的人声/背景声做二次混音, 故其输入全部来自
/// `separate` 步骤产出的 stems 目录(均为 `{video_dir}/separate/...` 的**约定拼法**,
/// 非 ctx 事实), 入口走 `from_video_dir` 而非 `from_state`:
/// - `vocals_path`: `target_3_vocals.wav`(纯人声), 混音 / gate 的输入;
/// - `stems_dir`:   `separate/` 目录, BGM 重建(drums/bass/other 三路)的来源。
pub struct SeparateAfterInput {
    /// separate 步骤输出的 vocals stem（人声），混音 / gate 的输入。
    pub vocals_path: separate::Target3VocalsPath,
    /// separate 步骤输出的 stems 目录（drums/bass/other/vocals），BGM 重建的来源。
    pub stems_dir: separate::StemsDir,
}

impl SeparateAfterInput {
    /// 全部为 `{video_dir}/separate/...` 的约定拼法, 故走 `from_video_dir`。
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self {
            vocals_path: separate::Target3VocalsPath::new(&video_dir),
            stems_dir: separate::StemsDir::new(&video_dir),
        }
    }
}
