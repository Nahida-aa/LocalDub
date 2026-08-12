//! pipeline 各阶段实现 (镜像 TS `packages/core/stages/`)
//!
//! 目前仅 split_audio 落地; 其余阶段 (asr / translate / tts / merge_audio / merge_video ...)
//! 待逐个移植。

pub mod split_audio;
