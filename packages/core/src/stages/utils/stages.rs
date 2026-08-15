//! pipeline 阶段序列定义与解析 (镜像 TS `packages/core/stages/utils/stages.ts`)
//!
//! TS 侧 `getStages` 通过 `readInputArgs()` 读取 subtitleSource / translate.enabled /
//! split_audio.vadAlign; Rust 侧没有该全局 singleton, 改为从 [`crate::context::TaskCtx`]
//! 的 `input` (已是 JSON Value) 解析相同字段。

use crate::context::TaskCtx;

/// 所有合法 stage 名 (镜像 TS `stagesList`)
pub const STAGES_LIST: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_fix",
    "sf_ocr_pre",
    "sf_ocr",
    "sf_ocr_fix",
    "asr_ocr_pre",
    "asr_ocr",
    "asr_ocr_fix",
    "translate",
    "split_audio",
    "tts",
    "mix_audio",
    "mix_video",
];

pub const DUB_STAGES: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_fix",
    "translate",
    "split_audio",
    "tts",
    "mix_audio",
    "mix_video",
];

pub const DUB_SF_OCR_STAGES: &[&str] = &[
    "separate",
    "separate_after",
    "sf_ocr_pre",
    "sf_ocr",
    "sf_ocr_fix",
    "translate",
    "split_audio",
    "tts",
    "mix_audio",
    "mix_video",
];

pub const DUB_ASR_OCR_STAGES: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_ocr_pre",
    "asr_ocr",
    "asr_ocr_fix",
    "translate",
    "split_audio",
    "tts",
    "mix_audio",
    "mix_video",
];

pub const SUBTITLE_STAGES: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_fix",
    "translate",
    "split_audio",
    "mix_video",
];

/// 从 ctx.input 解析 subtitleSource (缺省 "asr")
fn subtitle_source(ctx: &TaskCtx) -> String {
    ctx.input
        .get("task")
        .and_then(|v| v.get("subtitleSource"))
        .and_then(|v| v.as_str())
        .unwrap_or("asr")
        .to_string()
}

/// 从 ctx.input 解析 translate.enabled (缺省 true → 不剔除)
fn translate_enabled(ctx: &TaskCtx) -> bool {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("translate"))
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 从 ctx.input 解析 split_audio.vadAlign (缺省 false)
fn split_audio_vad_align(ctx: &TaskCtx) -> bool {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("split_audio"))
        .and_then(|v| v.get("vadAlign"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 根据 pipeline 与 subtitleSource / 开关过滤, 返回本次要执行的 stage 序列
/// (镜像 TS `getStages`)。
pub fn get_stages(ctx: &TaskCtx) -> Vec<String> {
    let is_subtitle = ctx.pipeline == "subtitle";
    let mut stages: Vec<String> = if is_subtitle {
        SUBTITLE_STAGES.iter().map(|s| s.to_string()).collect()
    } else {
        // dub 模式下按 subtitleSource 选基础序列
        let base = match subtitle_source(ctx).as_str() {
            "sf_ocr" => DUB_SF_OCR_STAGES,
            "asr_ocr" => DUB_ASR_OCR_STAGES,
            _ => DUB_STAGES,
        };
        base.iter().map(|s| s.to_string()).collect()
    };

    if !translate_enabled(ctx) {
        stages.retain(|s| s != "translate");
    }
    if is_subtitle && !split_audio_vad_align(ctx) {
        stages.retain(|s| s != "split_audio");
    }
    stages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx(pipeline: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = "/x".into();
        ctx.pipeline = pipeline.into();
        ctx
    }

    #[test]
    fn dub_default_is_asr() {
        let c = ctx(
            "dub",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert_eq!(
            get_stages(&c),
            vec![
                "separate",
                "separate_after",
                "asr",
                "asr_fix",
                "translate",
                "split_audio",
                "tts",
                "mix_audio",
                "mix_video"
            ]
        );
    }

    #[test]
    fn dub_sf_ocr() {
        let c = ctx(
            "dub",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"task": {"subtitleSource": "sf_ocr"}}
            }),
        );
        assert!(get_stages(&c).contains(&"sf_ocr".to_string()));
        assert!(!get_stages(&c).contains(&"asr".to_string()));
    }

    #[test]
    fn dub_asr_ocr() {
        let c = ctx(
            "dub",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"task": {"subtitleSource": "asr_ocr"}}
            }),
        );
        let s = get_stages(&c);
        assert!(s.contains(&"asr_ocr".to_string()));
        assert!(s.contains(&"asr_ocr_fix".to_string()));
    }

    #[test]
    fn translate_disabled_removes_stage() {
        let c = ctx(
            "dub",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"translate": {"enabled": false}}}
            }),
        );
        assert!(!get_stages(&c).contains(&"translate".to_string()));
    }

    #[test]
    fn subtitle_default_omits_split_audio_unless_vad_align() {
        let c = ctx(
            "subtitle",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert!(!get_stages(&c).contains(&"split_audio".to_string()));

        let c2 = ctx(
            "subtitle",
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"split_audio": {"vadAlign": true}}}
            }),
        );
        assert!(get_stages(&c2).contains(&"split_audio".to_string()));
    }
}
