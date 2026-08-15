//! pipeline 各阶段实现 (镜像 TS `packages/core/stages/`)
//!
//! [`utils`] 提供共享基础设施 (set_stage / 路径 helper / 日志);
//! 各阶段模块逐个移植, 目前 `separate` 已完成处理逻辑。

pub mod asr;
pub mod asr_ocr;
pub mod mix_audio;
pub mod mix_video;
pub mod separate;
pub mod sf_ocr;
pub mod split_audio;
pub mod translate;
pub mod tts;
pub mod utils;
