//! split_audio: 按字幕/翻译时间轴把音频切块, 供 tts 逐段合成。
//!
//! 镜像 TS `packages/core/stages/06_split_audio/`。目前是占位状态:
//! - 数据结构已落地 ([`types`])
//! - 配置读取已落地 ([`read_config`]), 对齐 TS `SplitAudioCliInputSchema`
//! - 主逻辑 `stage_split_audio` 尚未移植 (返回明确错误)

pub mod out;

use crate::context::TaskCtx;

/// split_audio 阶段配置 (镜像 TS `SplitAudioCliInputSchema`)
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SplitAudioConfig {
    /// 是否启用静音检测对齐 (修正 segments 前后静音导致的偏移)
    #[serde(default)]
    pub vad_align: bool,
    /// 人声文件路径, 调试使用
    pub vocals_file_path: Option<String>,
    /// 原始视频音频路径, 调试使用
    pub source_file_path: Option<String>,
}

/// 从 `ctx.input.stages.split_audio` 解析配置, 不存在时返回默认 (与 TS default 对齐)
pub fn read_config(ctx: &TaskCtx) -> SplitAudioConfig {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("split_audio"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 占位入口 (镜像 TS `stageSplitAudio`)
///
/// TODO: 尚未移植。待迁移: padSegments / ffmpeg 切块 / vadAlign 静音检测对齐 /
/// writeJson 输出 split_audio.json + timings.json / setStage 完成标记。
pub fn stage_split_audio(ctx: &TaskCtx) -> Result<(), String> {
    let cfg = read_config(ctx);
    Err(format!(
        "split_audio 尚未移植 (vadAlign={}, vocalsFilePath={:?})",
        cfg.vad_align, cfg.vocals_file_path
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_with_input(input: serde_json::Value) -> TaskCtx {
        crate::context::read_ctx_from_value(input).unwrap()
    }

    #[test]
    fn read_config_defaults_when_absent() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"}
        }));
        let cfg = read_config(&ctx);
        assert!(!cfg.vad_align);
        assert!(cfg.vocals_file_path.is_none());
        assert!(cfg.source_file_path.is_none());
    }

    #[test]
    fn read_config_parses_camel_case_fields() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"stages": {"split_audio": {
                "vadAlign": true,
                "vocalsFilePath": "/v.wav",
                "sourceFilePath": "/s.mp4"
            }}}
        }));
        let cfg = read_config(&ctx);
        assert!(cfg.vad_align);
        assert_eq!(cfg.vocals_file_path.as_deref(), Some("/v.wav"));
        assert_eq!(cfg.source_file_path.as_deref(), Some("/s.mp4"));
    }
}
