//! separate_after 步骤的跨步骤依赖清单(代码即 DAG 视角)。

use crate::steps::asr::args::AsrArgs;
use crate::steps::utils::paths::separate;

/// separate_after 步骤的跨步骤依赖清单。
///
/// 与 `AsrOcrPreInput` 等兄弟 `*Input` 同款定位: 只声明**被消费的跨步骤产物**,
/// 自身输出(stems / bgm / mixed / gated)不在清单内，`AsrArgs` 是本次运行参数。
///
/// separate_after 直接对 `separate` 的人声/背景声做二次混音, 故其输入全部来自
/// `separate` 步骤产出的 stems 目录(均为 `{video_dir}/separate/...` 的**约定拼法**,
/// 非拍平的全局状态):
/// - `vocals_path`: `target_3_vocals.wav`(纯人声), 混音 / gate 的输入;
/// - `stems_dir`:   `separate/` 目录, BGM 重建(drums/bass/other 三路)的来源。
///
/// `args` 是 asr 的配置（`input.steps.asr`）——sidechain / gate 这些开关由 asr
/// 决定（它们影响的是**喂给 asr 的人声**），而非本步自己的参数。
#[derive(Debug, Clone)]
pub struct SeparateAfterInput {
    /// 任务目录（写侧 step status 与产出路径需要）。
    pub video_dir: String,
    /// separate 步骤输出的 vocals stem（人声），混音 / gate 的输入。
    pub vocals_path: separate::Target3VocalsPath,
    /// separate 步骤输出的 stems 目录（drums/bass/other/vocals），BGM 重建的来源。
    pub stems_dir: separate::StemsDir,
    /// 本次运行参数（`input.steps.asr`：sidechain / gate 开关）。
    pub args: AsrArgs,
}

impl SeparateAfterInput {
    /// stems 全为 `{video_dir}/separate/...` 的约定拼法，故只需 `(video_dir, input)`。
    pub fn new(video_dir: &str, input: &crate::input::Input) -> Self {
        Self {
            video_dir: video_dir.to_string(),
            vocals_path: separate::Target3VocalsPath::new(video_dir),
            stems_dir: separate::StemsDir::new(video_dir),
            args: input.steps.asr.clone(),
        }
    }
}
