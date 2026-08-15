//! asr 阶段参数 (镜像 TS `packages/core/stages/asr/args.ts` AsrArgsSchema)。
//!
//! 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
//! `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
//! 以保证两种路径下默认值一致 (与 `input::stages::Asr` 同款处理)。

use serde::{Deserialize, Serialize};

/// asr 推理运行时。
///
/// TS 侧为 `z.enum(["ggml", "faster-whisper", "pytorch"])` (kebab-case),
/// 故用 `rename_all = "kebab-case"` (`FasterWhisper` → `"faster-whisper"`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    Ggml,
    FasterWhisper,
    #[default]
    Pytorch,
}

/// 推理设备 (asr 侧无 webgpu)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum AsrDevice {
    Vulkan,
    #[default]
    Cuda,
    Cpu,
    Mps,
}

/// ASR 音频源: vocals=纯分离人声, raw-sum=直接叠加, sidechain=侧链压缩背景音
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MixMode {
    Vocals,
    #[serde(rename = "raw-sum")]
    RawSum,
    #[default]
    Sidechain,
}

/// 侧链压缩器参数 (镜像 TS `sidechainCompress`)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SidechainCompress {
    /// 压缩器阈值, 默认 0.1
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// 压缩比, 默认 20
    #[serde(default = "default_ratio")]
    pub ratio: f64,
    /// attack 时间(ms), 默认 1
    #[serde(default = "default_attack")]
    pub attack: f64,
    /// release 时间(ms), 默认 200
    #[serde(default = "default_release")]
    pub release: f64,
}

impl Default for SidechainCompress {
    fn default() -> Self {
        Self {
            threshold: default_threshold(),
            ratio: default_ratio(),
            attack: default_attack(),
            release: default_release(),
        }
    }
}

fn default_threshold() -> f64 {
    0.1
}

fn default_ratio() -> f64 {
    20.0
}

fn default_attack() -> f64 {
    1.0
}

fn default_release() -> f64 {
    200.0
}

/// whisper.cpp VAD 模型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum VadModel {
    SileroV5,
    SileroV6,
}

/// asr 阶段参数。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrArgs {
    /// 推理运行时, 默认 pytorch
    #[serde(default = "default_runtime")]
    pub runtime: Runtime,
    /// 推理设备, 默认 cuda
    #[serde(default = "default_device")]
    pub device: AsrDevice,
    /// 使用分离后的人声 (target_3_vocals.wav) 而非原始视频音频
    #[serde(default)]
    pub use_separated: bool,
    /// ASR 音频源 (默认 sidechain)
    #[serde(default = "default_mix_mode")]
    pub mix_mode: MixMode,
    /// 背景音降低量(dB); raw-sum 叠加前衰减, sidechain 时压缩后额外衰减; 默认 -12
    #[serde(default = "default_reduce_bgm")]
    pub reduce_bgm: f64,
    /// 是否在 asr.json 中包含词级时间戳 (words), 分离场景下可能受幻觉影响; 默认关闭, 调试时开启
    #[serde(default)]
    pub words_output: bool,
    /// mixMode=sidechain 时侧链压缩器参数
    #[serde(default = "default_sidechain_compress")]
    pub sidechain_compress: SidechainCompress,
    /// 对分离后的人声应用 silence gate 过滤静音段噪声
    #[serde(default)]
    pub use_gate: bool,
    /// ASR 输入的人声音频路径, 调试使用
    #[serde(default)]
    pub vocal_audio_path: Option<String>,
    /// whisper.cpp: 启用 VAD
    #[serde(default)]
    pub vad: bool,
    /// whisper.cpp: VAD 模型, silero-v5 (默认) 或 silero-v6
    #[serde(default)]
    pub vad_model: Option<VadModel>,
    /// whisper.cpp: VAD 阈值, 默认 0.5
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f64,
    /// whisper.cpp: no-speech 阈值, 默认 0.6
    #[serde(default = "default_no_speech_thold")]
    pub no_speech_thold: f64,
    /// whisper.cpp: 解码温度, 默认 0.0
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// whisper.cpp: 最大段长(字符), 0=不限
    #[serde(default = "default_max_len")]
    pub max_len: u32,
    /// whisper.cpp: 按词边界分割
    #[serde(default)]
    pub split_on_word: bool,
}

impl Default for AsrArgs {
    fn default() -> Self {
        Self {
            runtime: default_runtime(),
            device: default_device(),
            use_separated: false,
            mix_mode: default_mix_mode(),
            reduce_bgm: default_reduce_bgm(),
            words_output: false,
            sidechain_compress: default_sidechain_compress(),
            use_gate: false,
            vocal_audio_path: None,
            vad: false,
            vad_model: None,
            vad_threshold: default_vad_threshold(),
            no_speech_thold: default_no_speech_thold(),
            temperature: default_temperature(),
            max_len: default_max_len(),
            split_on_word: false,
        }
    }
}

fn default_runtime() -> Runtime {
    Runtime::Pytorch
}

fn default_device() -> AsrDevice {
    AsrDevice::Cuda
}

fn default_mix_mode() -> MixMode {
    MixMode::Sidechain
}

fn default_reduce_bgm() -> f64 {
    -12.0
}

fn default_sidechain_compress() -> SidechainCompress {
    SidechainCompress::default()
}

fn default_vad_threshold() -> f64 {
    0.5
}

fn default_no_speech_thold() -> f64 {
    0.6
}

fn default_temperature() -> f64 {
    0.0
}

fn default_max_len() -> u32 {
    0
}
