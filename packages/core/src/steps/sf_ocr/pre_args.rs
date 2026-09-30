//! sf_ocr_pre 阶段输入契约。

use crate::steps::utils::paths::VideoSourcePath;

/// sf_ocr_pre 步骤的输入契约（**依赖清单**）：本步**消费**的路径。
///
/// 只装依赖——原视频文件 `video_source.mp4`。本步产出（`sf_ocr_pre/` 下的
/// frames/ mask/ keyframes.json）属于「产出」而非「依赖」，不在此清单内。与
/// `SeparateAfterInput` / `TtsInput` 同款：`StepInput` 是依赖清单。
///
/// 视频源是**约定拼法**（import 固定落在 `{video_dir}/video_source.mp4`），故入口
/// 是 [`new`](Self::new)`(video_dir, input)` 而非从状态里读副本——与
/// [`VideoSourcePath::from_video_dir`] 同款。
pub struct SfOcrPreInput {
    /// 任务目录（写侧 step status 与产出路径需要）。
    pub video_dir: String,
    /// 原视频文件（工作流级输入，由 import 产出）。
    pub video_source_path: VideoSourcePath,
}

impl SfOcrPreInput {
    pub fn new(video_dir: &str, _input: &crate::input::Input) -> Self {
        Self {
            video_dir: video_dir.to_string(),
            video_source_path: VideoSourcePath::from_video_dir(video_dir),
        }
    }
}
