//! pipeline 各阶段实现 (镜像 TS `packages/core/stages/`)
//!
//! 目前已落地: split_audio / separate / translate (数据结构 + 配置读取);
//! 其余阶段 (asr / tts / mix_audio / mix_video ...) 待逐个移植。

pub mod separate;
pub mod split_audio;
pub mod translate;
pub mod tts;
