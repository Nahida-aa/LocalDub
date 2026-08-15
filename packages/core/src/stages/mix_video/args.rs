use serde::{Deserialize, Serialize};

/// 字幕对齐位置 (镜像 TS `packages/core/stages/mix_video/args.ts` alignmentList)
///
/// 顺序即 ffmpeg ass `Alignment` 数值 (1..9), 见 [`alignment_to_ffmpeg`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Alignment {
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
    MiddleLeft,
    Center,
    MiddleRight,
    TopLeft,
    TopCenter,
    TopRight,
}

/// 对齐位置 → ffmpeg ass `Alignment` 数值 (1..9)
pub fn alignment_to_ffmpeg(alignment: Alignment) -> u8 {
    match alignment {
        Alignment::BottomLeft => 1,
        Alignment::BottomCenter => 2,
        Alignment::BottomRight => 3,
        Alignment::MiddleLeft => 4,
        Alignment::Center => 5,
        Alignment::MiddleRight => 6,
        Alignment::TopLeft => 7,
        Alignment::TopCenter => 8,
        Alignment::TopRight => 9,
    }
}

/// mix_video 阶段参数 (镜像 TS `packages/core/stages/mix_video/args.ts` MixVideoArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故这里手写 `impl Default`
/// 以保证两种路径下默认值一致 (与 `input::stages::Asr` 同款处理)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MixVideoArgs {
    /// 字幕字号, 不填则自动: 竖屏 12(zh)/9(其他) ← 横屏 24(zh)/18(其他)
    #[serde(default)]
    pub font_size: Option<u32>,
    /// 垂直边距(像素), 不填则自动: 竖屏 70 / 横屏 5
    #[serde(default)]
    pub margin_v: Option<u32>,
    /// 对齐位置; 默认 bottom-center
    #[serde(default)]
    pub alignment: Option<Alignment>,
    /// 描边宽度; 默认 0
    #[serde(default = "default_outline")]
    pub outline: u32,
    /// 阴影宽度; 默认 1
    #[serde(default = "default_shadow")]
    pub shadow: u32,
    /// ASS 字幕字体名 (须系统已安装), 默认 Noto Sans CJK SC
    #[serde(default)]
    pub font: Option<String>,
    /// 调试使用: 外部 srt 路径
    #[serde(default)]
    pub srt_path: Option<String>,
    /// 调试使用: 外部 bgm 路径
    #[serde(default)]
    pub bgm_path: Option<String>,
    /// 背景音乐增益(dB), 0=不变, 负值=衰减; 默认 -6
    #[serde(default = "default_bgm_gain")]
    pub bgm_gain: f64,
    /// 配音增益(dB), 补偿合成语音偏小的听感差; 默认 3
    #[serde(default = "default_dub_gain")]
    pub dub_gain: f64,
    /// 是否启用本阶段 (缺省 true; 设为 false 可跳过 mix_video)
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for MixVideoArgs {
    fn default() -> Self {
        Self {
            font_size: None,
            margin_v: None,
            alignment: None,
            outline: default_outline(),
            shadow: default_shadow(),
            font: None,
            srt_path: None,
            bgm_path: None,
            bgm_gain: default_bgm_gain(),
            dub_gain: default_dub_gain(),
            enabled: default_enabled(),
        }
    }
}

fn default_outline() -> u32 {
    0
}

fn default_shadow() -> u32 {
    1
}

fn default_bgm_gain() -> f64 {
    -6.0
}

fn default_dub_gain() -> f64 {
    3.0
}
