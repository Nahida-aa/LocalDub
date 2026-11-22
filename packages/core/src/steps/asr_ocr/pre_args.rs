//! asr_ocr 阶段预处理参数 (镜像 TS `packages/core/steps/04_asr_ocr/pre_args.ts`)。

use crate::steps::utils::paths::asr;
use serde::{Deserialize, Serialize};

/// asr_ocr 阶段预处理参数。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::steps::Asr` 同款处理)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
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

/// Phase 1 事实面：`step_asr_ocr_pre` 的显式依赖清单（由装配层算好后注入）。
///
/// 装本步消费的跨步骤产物：`asr_path`——asr 步骤产出的 `asr/asr.json`，本步据此
/// 做 ASR 切分。
///
/// 原视频路径（import 产出）由步骤体从 `video_dir` 约定拼出，不进 deps——它与
/// `video_dir` 同源，不需要单独声明为依赖。
///
/// 意图面（`video_dir`）走 `sc.ctx().input()`，不进 deps。
/// 本步的产出（`asr_ocr_pre/asr_split.json` / `frames/`）见 `pre_out::AsrOcrPreOutput`。
#[derive(Debug, Clone)]
pub struct AsrOcrPreDeps {
    /// asr 步骤产出的识别结果 `asr/asr.json`。
    pub asr_path: asr::AsrPath,
}

impl AsrOcrPreDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        Self {
            asr_path: asr::AsrPath::new(input.video_dir()),
        }
    }
}
