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
mod translate {
    use super::*;
    // translate/{lang}.json
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
