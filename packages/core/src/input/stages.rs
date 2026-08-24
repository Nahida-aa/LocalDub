use serde::{Deserialize, Serialize};

use crate::stages::asr::args::AsrArgs;
use crate::stages::asr::fix_args::AsrFixArgs;
use crate::stages::asr_ocr::args::AsrOcrArgs;
use crate::stages::asr_ocr::fix_args::AsrOcrFixArgs;
use crate::stages::asr_ocr::pre_args::AsrOcrPreArgs;
use crate::stages::mix_audio::args::MixAudioArgs;
use crate::stages::mix_video::args::MixVideoArgs;
use crate::stages::separate::SeparateArgs;
use crate::stages::sf_ocr::args::SfOcrArgs;
use crate::stages::sf_ocr::fix_args::OcrFixArgs;
use crate::stages::split_audio::SplitAudioArgs;
use crate::stages::translate::args::TranslateArgs;
use crate::stages::tts::args::TtsArgs;

/// 各处理阶段的入参 (镜像 TS `packages/core/input/types.ts` StagesSchema)
///
/// mix_audio / mix_video 直接使用 `crate::stages::{mix_audio,mix_video}::args` 的
/// 完整定义，与 TS `MixAudioArgsSchema` / `MixVideoArgsSchema` 对齐。
/// sf_ocr / asr_ocr 系列同样复用各自 stage 目录下的 args 定义。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Stages {
    #[serde(default)]
    pub separate: SeparateArgs,
    #[serde(default)]
    pub asr: AsrArgs,
    #[serde(default)]
    pub asr_fix: AsrFixArgs,
    #[serde(default)]
    pub sf_ocr: SfOcrArgs,
    #[serde(default)]
    pub sf_ocr_fix: OcrFixArgs,
    #[serde(default)]
    pub asr_ocr_pre: AsrOcrPreArgs,
    #[serde(default)]
    pub asr_ocr: AsrOcrArgs,
    #[serde(default)]
    pub asr_ocr_fix: AsrOcrFixArgs,
    #[serde(default)]
    pub translate: TranslateArgs,
    #[serde(default)]
    pub split_audio: SplitAudioArgs,
    #[serde(default)]
    pub tts: TtsArgs,
    #[serde(default)]
    pub mix_audio: MixAudioArgs,
    #[serde(default)]
    pub mix_video: MixVideoArgs,
}

impl Default for Stages {
    fn default() -> Self {
        Self {
            separate: SeparateArgs::default(),
            asr: AsrArgs::default(),
            asr_fix: AsrFixArgs::default(),
            sf_ocr: SfOcrArgs::default(),
            sf_ocr_fix: OcrFixArgs::default(),
            asr_ocr_pre: AsrOcrPreArgs::default(),
            asr_ocr: AsrOcrArgs::default(),
            asr_ocr_fix: AsrOcrFixArgs::default(),
            translate: TranslateArgs::default(),
            split_audio: SplitAudioArgs::default(),
            tts: TtsArgs::default(),
            mix_audio: MixAudioArgs::default(),
            mix_video: MixVideoArgs::default(),
        }
    }
}
