//! asr_ocr 阶段预处理参数 (镜像 TS `packages/core/stages/04_asr_ocr/pre_args.ts`)。

use serde::{Deserialize, Serialize};

/// asr_ocr 阶段预处理参数。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrOcrPreArgs {
    /// 帧率 (fps), 越高时间戳越准但越慢; 默认 2
    #[serde(default = "default_fps")]
    pub fps: f64,
}

fn default_fps() -> f64 {
    2.0
}
