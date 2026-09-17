//! sf_ocr_pre 阶段输入契约。

use crate::state::WorkflowState;
use crate::steps::utils::paths::VideoSourcePath;

/// sf_ocr_pre 步骤的输入契约（**依赖清单**）：本步**消费**的路径。
///
/// 只装依赖——原视频文件 `video_source.mp4`（由 import 阶段写入 ctx 的事实，
/// 非 video_dir 约定拼法）。本步产出（`sf_ocr_pre/` 下的 frames/ mask/ keyframes.json）
/// 属于「产出」而非「依赖」，不在此清单内。与 `SfOcrFixInput` / `TtsInput` 同款：
/// `StepInput` 是依赖清单。
///
/// 视频源是 ctx 事实（import 落盘后写进 `ctx.video_source_path`），故入口是
/// [`from_state`](Self::from_state) 而非 `from_video_dir`——与
/// [`VideoSourcePath::from_state`] 同款：缺失即报错（没导入过就没有这个文件）。
/// 这也是 `MixVideoInput` 的取法（见 `mix_video/args.rs`）。
pub struct SfOcrPreInput {
    /// 原视频文件（任务级输入，由 import 产出）。
    pub video_source_path: VideoSourcePath,
}

impl SfOcrPreInput {
    /// 从工作流状态解析本步输入契约（视频源是 ctx 事实）。
    pub fn from_state(state: &WorkflowState) -> anyhow::Result<Self> {
        Ok(Self {
            video_source_path: VideoSourcePath::from_state(state)?,
        })
    }
}
