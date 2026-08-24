use serde::{Deserialize, Serialize};

/// mix_audio 阶段参数 (镜像 TS `packages/core/stages/mix_audio/args.ts` MixAudioArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::stages::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MixAudioArgs {
    /// TTS 音频最大变速比, 1.0=不变速; 默认 1.35
    #[serde(default = "default_max_speed")]
    pub max_speed: f64,
    /// 字幕允许提前显示的最大毫秒数, 利用前段剩余时间; 默认 500
    #[serde(default = "default_max_advance_ms")]
    pub max_advance_ms: f64,
    /// 字幕允许延迟显示的最大毫秒数, 借用后段留白; 默认 500
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: f64,
}

impl Default for MixAudioArgs {
    fn default() -> Self {
        Self {
            max_speed: default_max_speed(),
            max_advance_ms: default_max_advance_ms(),
            max_delay_ms: default_max_delay_ms(),
        }
    }
}

fn default_max_speed() -> f64 {
    1.35
}

fn default_max_advance_ms() -> f64 {
    500.0
}

fn default_max_delay_ms() -> f64 {
    500.0
}
