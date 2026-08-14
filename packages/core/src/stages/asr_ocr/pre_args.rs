//! asr_ocr 阶段预处理参数 (镜像 TS `packages/core/stages/04_asr_ocr/pre_args.ts`)。

use serde::{Deserialize, Serialize};

/// asr_ocr 阶段预处理参数。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::stages::Asr` 同款处理)。
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
