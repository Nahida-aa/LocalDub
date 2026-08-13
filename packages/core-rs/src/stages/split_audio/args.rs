use serde::{Deserialize, Serialize};

/// split_audio 阶段参数 (镜像 TS `packages/core/stages/06_split_audio/args.ts` SplitAudioArgsSchema)
///
/// 默认值带业务含义 (startPadMs=100 / endPadMs=300)。TS 端 zod `.prefault({})`
/// 在写入 ctx.json 前已落定默认值, 因此这里只需处理「对象存在但字段缺」:
/// 用 `#[serde(default = "…")]` 兜底, 不必手写 `impl Default`。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SplitAudioArgs {
    /// 是否启用静音检测对齐: 修正 segments 前后静音导致的偏移
    #[serde(default)]
    pub vad_align: bool,
    /// 段落切块前缘 padding (ms), 避免语音被截断
    #[serde(default = "default_start_pad_ms")]
    pub start_pad_ms: u64,
    /// 段落切块后缘 padding (ms), 避免语音被截断
    #[serde(default = "default_end_pad_ms")]
    pub end_pad_ms: u64,
    /// 人声文件路径, 调试使用
    pub vocals_file_path: Option<String>,
    /// 原始视频音频路径, 调试使用
    pub source_file_path: Option<String>,
}

fn default_start_pad_ms() -> u64 {
    100
}

fn default_end_pad_ms() -> u64 {
    300
}
