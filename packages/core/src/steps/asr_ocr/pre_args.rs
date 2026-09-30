//! asr_ocr 阶段预处理参数 (镜像 TS `packages/core/steps/04_asr_ocr/pre_args.ts`)。

use crate::steps::utils::paths::{asr, VideoSourcePath};
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段预处理参数。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrOcrPreArgs {
    /// 帧率 (fps), 越高时间戳越准但越慢; 默认 2
    #[serde(default = "default_fps")]
    pub fps: f64,
}

impl Default for AsrOcrPreArgs {
    fn default() -> Self {
        Self { fps: default_fps() }
    }
}

fn default_fps() -> f64 {
    2.0
}

/// asr_ocr_pre 步骤的输入契约（**依赖清单**）：本步**消费**的路径。
///
/// 装三条东西：
/// - `asr/asr.json`（`asr` 步骤产出，约定拼法 `{video_dir}/asr/asr.json`，
///   经 [`asr::AsrPath`]）——本步据此做 ASR 切分；
/// - 原视频文件 `video_source.mp4`（约定拼法，经
///   [`VideoSourcePath::from_video_dir`]）；
/// - `args`：本次抽帧参数（`input.steps.asr_ocr_pre`）。
///
/// 两条路径都是 `{video_dir}/...` 的约定拼法，故入口是
/// [`new`](Self::new)`(video_dir, input)`。
///
/// 本步的产出（`asr_ocr_pre/asr_split.json` / `asr_ocr_pre/frames/`）属于「产出」而非
/// 「依赖」，不在此清单内。与 `SfOcrPreInput` / `TtsInput` 同款：
/// `StepInput` 是依赖清单。
pub struct AsrOcrPreInput {
    /// 任务目录（写侧 step status 需要）。
    pub video_dir: String,
    /// 来自 asr 步骤的识别结果 `asr/asr.json`。
    pub asr_path: asr::AsrPath,
    /// 原视频文件（任务级输入，由 import 产出）。
    pub video_source_path: VideoSourcePath,
    /// 本次运行参数（`input.steps.asr_ocr_pre`：抽帧帧率）。
    pub args: AsrOcrPreArgs,
}

impl AsrOcrPreInput {
    pub fn new(video_dir: &str, input: &crate::input::Input) -> Self {
        Self {
            video_dir: video_dir.to_string(),
            asr_path: asr::AsrPath::new(video_dir),
            video_source_path: VideoSourcePath::from_video_dir(video_dir),
            args: input.steps.asr_ocr_pre.clone(),
        }
    }
}
