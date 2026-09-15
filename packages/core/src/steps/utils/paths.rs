//! 按 step 分命名空间的类型化路径词汇表。
//!
//! **现状：多数别名尚无消费者。** 目前真正被外部引用（`TranslateInput` /
//! `TranslateOutput`）的只有 `asr_fix` / `sf_ocr_fix` / `asr_ocr_fix` 的
//! `SrtPath` 与 `translate::LangPath`；其余是**为后续 step 契约预留的词汇表**。
//! 读到它们时不要假设「有人在用」——路径的实际拼装目前仍在
//! `utils/mod.rs` 的 `*_path()` 函数里。
//!
//! 别名的价值在于**让 provenance 进类型**（`asr_fix::SrtPath` 一眼看出是谁写的），
//! 而不是包装 `PathBuf` 本身。
//!
//! `dead_code` 在此**刻意放行**：这个文件是词汇表，未使用的条目不是疏漏而是待用。
//! （注意：`translate::LangPath` 转成 `pub mod` 后，编译器才开始检查本文件的
//! 其余条目——此前它们在私有模块里，分析被跳过。）
#![allow(dead_code)]

use std::path::PathBuf;

// video_source.mp4
type VideoSourcePath = PathBuf;

mod separate_after {
    use super::*;
    // separate_after/target_bgm.wav
    pub type TargetBgmPath = PathBuf;
    // separate_after/target_3_vocals_mixed.wav
    pub type Target3VocalsMixedPath = PathBuf;
}

mod asr {
    use super::*;
    // asr/asr.json
    pub type AsrPath = PathBuf;
}

pub mod asr_fix {
    use super::*;
    // asr_fix/asr_fix.json
    pub type AsrFixPath = PathBuf;
    // asr_fix/srt.json
    pub type SrtPath = PathBuf;
}

pub mod sf_ocr_fix {
    use super::*;
    // sf_ocr_fix/srt.json
    pub type SrtPath = PathBuf;
}
pub mod asr_ocr_fix {
    use super::*;
    // asr_ocr_fix/srt.json
    pub type SrtPath = PathBuf;
}
pub mod translate {
    use super::*;
    // translate/translation.{lang}.json
    pub type LangPath = PathBuf;
}
mod split_audio {
    use super::*;
    // split_audio/split_audio.json
    pub type SplitAudioPath = PathBuf;
    // split_audio/timings.json
    pub type TimingsPath = PathBuf;
}
mod tts {
    use super::*;
    // tts/tts.json
    pub type TtsPath = PathBuf;
}
mod mix_audio {
    use super::*;
    // mix_audio/audio_dubbing.wav
    pub type DubbingPath = PathBuf;
    // mix_audio/timings.json
    pub type TimingsPath = PathBuf;
}

pub struct SeparateAfterOutput {
    target_3_vocals_mixed_path: separate_after::Target3VocalsMixedPath,
    target_bgm_path: separate_after::TargetBgmPath,
}
