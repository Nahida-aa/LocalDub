//! pipeline 各阶段实现 (镜像 TS `packages/core/stages/`)
//!
//! [`utils`] 提供共享基础设施 (set_stage / 路径 helper / 日志);
//! 各阶段模块逐个移植, 目前 `separate` 已完成处理逻辑。

pub mod asr;
pub mod asr_ocr;
pub mod mix_audio;
pub mod mix_video;
pub mod pipeline;
pub mod separate;
pub mod sf_ocr;
pub mod split_audio;
pub mod translate;
pub mod tts;
pub mod utils;

/// 根据 pipeline / subtitleSource / 开关解析本次要执行的 stage 序列。
pub use utils::stages::get_stages;

/// 串行运行 pipeline (镜像 TS `runPipeline`)。
pub use pipeline::run_pipeline;
