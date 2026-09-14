//! pipeline 阶段序列定义与解析 (镜像 TS `packages/core/steps/utils/steps.ts`)
//!
//! TS 侧 `getSteps` 通过 `readInputArgs()` 读取 subtitleSource / translate.enabled /
//! split_audio.vadAlign; Rust 侧没有该全局 singleton, 改为从 [`crate::context::WorkflowCtx`]
//! 的 `input` (已是 JSON Value) 解析相同字段。

use crate::context::WorkflowCtx;
use crate::workflows::args::StepName;

/// 所有合法 step 名 (镜像 TS `stepsList`)
pub const STEPS_LIST: &[&str] = &[
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

pub const DUB_STEPS: &[&str] = &[
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

pub const DUB_SF_OCR_STEPS: &[&str] = &[
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

pub const DUB_ASR_OCR_STEPS: &[&str] = &[
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

pub const SUBTITLE_STEPS: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_fix",
    "translate",
    "split_audio",
    "mix_video",
];

/// subtitle 模式 + subtitleSource=sf_ocr: OCR 提硬字幕 → 翻译 → 烧字幕, 不配音 (无 tts/mix_audio)。
pub const SUBTITLE_SF_OCR_STEPS: &[&str] = &[
    "separate",
    "separate_after",
    "sf_ocr_pre",
    "sf_ocr",
    "sf_ocr_fix",
    "translate",
    "split_audio",
    "mix_video",
];

/// subtitle 模式 + subtitleSource=asr_ocr: ASR 提时序 → OCR 校正文本 → 翻译 → 烧字幕, 不配音。
pub const SUBTITLE_ASR_OCR_STEPS: &[&str] = &[
    "separate",
    "separate_after",
    "asr",
    "asr_ocr_pre",
    "asr_ocr",
    "asr_ocr_fix",
    "translate",
    "split_audio",
    "mix_video",
];

/// 从 ctx.input 解析 subtitleSource (缺省 "asr")
fn subtitle_source(ctx: &WorkflowCtx) -> String {
    ctx.input
        .get("workflow")
        .and_then(|v| v.get("subtitleSource"))
        .and_then(|v| v.as_str())
        .unwrap_or("asr")
        .to_string()
}

/// 从 ctx.input 解析 translate.enabled (缺省 true → 不剔除)
fn translate_enabled(ctx: &WorkflowCtx) -> bool {
    ctx.input
        .get("steps")
        .and_then(|v| v.get("translate"))
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 从 ctx.input 解析 split_audio.vadAlign (缺省 false)
fn split_audio_vad_align(ctx: &WorkflowCtx) -> bool {
    ctx.input
        .get("steps")
        .and_then(|v| v.get("split_audio"))
        .and_then(|v| v.get("vadAlign"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 根据 pipeline 与 subtitleSource / 开关过滤, 返回本次要执行的 step 序列
/// (镜像 TS `getSteps`)。
///
/// 返回**已解析的 [`StepName`]**：序列常量仍是字符串（便于和 TS 逐字对照），
/// 但在这里立刻解析——**写错名字会 panic**，而不是流到后面被静默跳过。
/// 有 `get_steps_is_exhaustive` 测试兜底，所以 panic 只在开发期触发。
pub fn get_steps(ctx: &WorkflowCtx) -> Vec<StepName> {
    let is_subtitle = ctx.pipeline == "subtitle";
    let mut steps: Vec<&'static str> = if is_subtitle {
        // subtitle 模式: 按 subtitleSource 切换字幕提取策略, 但始终只到 mix_video
        // (不配音 → 无 tts / mix_audio)。subtitleSource 语义是"字幕怎么提取",
        // 不是"是否配音"; dub 序列常量含 tts/mix_audio, 不能复用。
        let base = match subtitle_source(ctx).as_str() {
            "sf_ocr" => SUBTITLE_SF_OCR_STEPS,
            "asr_ocr" => SUBTITLE_ASR_OCR_STEPS,
            _ => SUBTITLE_STEPS,
        };
        base.to_vec()
    } else {
        // dub 模式下按 subtitleSource 选基础序列
        let base = match subtitle_source(ctx).as_str() {
            "sf_ocr" => DUB_SF_OCR_STEPS,
            "asr_ocr" => DUB_ASR_OCR_STEPS,
            _ => DUB_STEPS,
        };
        base.to_vec()
    };

    if !translate_enabled(ctx) {
        steps.retain(|s| *s != "translate");
    }
    if is_subtitle && !split_audio_vad_align(ctx) {
        steps.retain(|s| *s != "split_audio");
    }

    // 解析成枚举——pipeline 常量与 `StepName` 必须一致，不一致说明有一处改名漏了。
    steps
        .into_iter()
        .map(|s| {
            StepName::parse(s).unwrap_or_else(|| {
                panic!("pipeline 序列里的 step \"{s}\" 不是合法的 StepName（改名时漏了某处？）")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use crate::workflows::args::ALL_STEPS;
    use serde_json::json;

    fn ctx(pipeline: &str, input: serde_json::Value) -> WorkflowCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.video_dir = "/x".into();
        ctx.pipeline = pipeline.into();
        ctx
    }

    /// 断言用：把 `Vec<StepName>` 转成名字列表，测试里仍可逐字写字符串。
    fn names(steps: &[StepName]) -> Vec<&'static str> {
        steps.iter().map(|s| s.as_str()).collect()
    }

    /// 断言用：序列里有没有这个 step。
    fn has(steps: &[StepName], name: &str) -> bool {
        steps.iter().any(|s| s.as_str() == name)
    }

    /// **一致性护栏**：pipeline 常量里的每一步都必须是合法的 `StepName`。
    ///
    /// `get_steps` 内部解析失败会 panic，这里把所有常量都跑一遍，
    /// 改名漏一处时**这个测试会失败**（而不是等某条 pipeline 真的跑起来才炸）。
    #[test]
    fn all_pipeline_constants_are_valid_step_names() {
        for (label, seq) in [
            ("STEPS_LIST", STEPS_LIST),
            ("DUB_STEPS", DUB_STEPS),
            ("DUB_SF_OCR_STEPS", DUB_SF_OCR_STEPS),
            ("DUB_ASR_OCR_STEPS", DUB_ASR_OCR_STEPS),
            ("SUBTITLE_STEPS", SUBTITLE_STEPS),
            ("SUBTITLE_SF_OCR_STEPS", SUBTITLE_SF_OCR_STEPS),
            ("SUBTITLE_ASR_OCR_STEPS", SUBTITLE_ASR_OCR_STEPS),
        ] {
            for s in seq {
                assert!(
                    StepName::parse(s).is_some(),
                    "{label} 里的 \"{s}\" 不是合法的 StepName（改名时漏了某处？）"
                );
            }
        }
    }

    /// `STEPS_LIST` 与 `ALL_STEPS` 必须覆盖同一批 step（两个名单不许漂移）。
    #[test]
    fn steps_list_matches_all_steps() {
        let mut from_list: Vec<&str> = STEPS_LIST.to_vec();
        let mut from_enum = names(ALL_STEPS);
        from_list.sort_unstable();
        from_enum.sort_unstable();
        assert_eq!(from_list, from_enum, "STEPS_LIST 与 ALL_STEPS 不一致");
    }

    /// **文档护栏**：`DEPENDENCIES.md` 必须为每个 step 都有一节，且每节都写了
    /// 「读」与「写」。
    ///
    /// 为什么需要：那份文档是给人/agent 读的依赖契约，但它**会悄悄过期**——
    /// 新增 step 忘了写、step 改名后文档没跟，都不会报错。这个测试把它变成
    /// 「过期就红」。
    ///
    /// 限度（**只覆盖"有没有"，不覆盖"对不对"**）：路径内容的正确性它管不了，
    /// 尤其是通过 helper 间接拼出来的路径（见文档「已知限制」）。
    #[test]
    fn dependencies_doc_covers_every_step() {
        let doc = include_str!("../DEPENDENCIES.md");

        // 收集 `### \`name\`` 形式的标题。
        let documented: Vec<String> = doc
            .lines()
            .filter_map(|l| l.strip_prefix("### `"))
            .filter_map(|l| l.strip_suffix('`'))
            .map(|s| s.to_string())
            .collect();

        let mut expected: Vec<String> = ALL_STEPS.iter().map(|s| s.to_string()).collect();
        expected.sort();
        let mut got = documented.clone();
        got.sort();

        assert_eq!(
            got, expected,
            "DEPENDENCIES.md 的 step 小节与 ALL_STEPS 不一致：\n\
             缺文档 = {missing:?}\n\
             多余文档 = {extra:?}",
            missing = expected
                .iter()
                .filter(|s| !got.contains(s))
                .collect::<Vec<_>>(),
            extra = got
                .iter()
                .filter(|s| !expected.contains(s))
                .collect::<Vec<_>>(),
        );

        // 每节里必须有「读」和「写」两行（表格首列）。
        for step in &expected {
            let body = section_body(&doc, step);
            assert!(
                body.contains("**读**"),
                "DEPENDENCIES.md 的 `{step}` 小节缺「读」行"
            );
            assert!(
                body.contains("**写**"),
                "DEPENDENCIES.md 的 `{step}` 小节缺「写」行"
            );
        }
    }

    /// 取出 `### \`name\`` 到下一个 `###` / `##` 之间的正文。
    fn section_body(doc: &str, name: &str) -> String {
        let marker = format!("### `{name}`");
        let Some(start) = doc.find(&marker) else {
            return String::new();
        };
        let rest = &doc[start + marker.len()..];
        let end = rest
            .find("\n## ")
            .into_iter()
            .chain(rest.find("\n### "))
            .min()
            .unwrap_or(rest.len());
        rest[..end].to_string()
    }

    #[test]
    fn dub_default_is_asr() {
        let c = ctx(
            "dub",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert_eq!(
            names(&get_steps(&c)),
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
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"subtitleSource": "sf_ocr"}}
            }),
        );
        assert!(has(&get_steps(&c), "sf_ocr"));
        assert!(!has(&get_steps(&c), "asr"));
    }

    #[test]
    fn dub_asr_ocr() {
        let c = ctx(
            "dub",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"subtitleSource": "asr_ocr"}}
            }),
        );
        let s = get_steps(&c);
        assert!(has(&s, "asr_ocr"));
        assert!(has(&s, "asr_ocr_fix"));
    }

    #[test]
    fn translate_disabled_removes_step() {
        let c = ctx(
            "dub",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"translate": {"enabled": false}}}
            }),
        );
        assert!(!has(&get_steps(&c), "translate"));
    }

    #[test]
    fn subtitle_default_omits_split_audio_unless_vad_align() {
        let c = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert!(!has(&get_steps(&c), "split_audio"));

        let c2 = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"split_audio": {"vadAlign": true}}}
            }),
        );
        assert!(has(&get_steps(&c2), "split_audio"));
    }

    #[test]
    fn subtitle_sf_ocr_uses_ocr_not_asr() {
        // subtitle + sf_ocr: OCR 提字幕, 不配音 (无 tts/mix_audio/audio), 也不走 asr 听写。
        let c = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"subtitleSource": "sf_ocr"}}
            }),
        );
        let s = get_steps(&c);
        assert!(has(&s, "sf_ocr_pre"));
        assert!(has(&s, "sf_ocr"));
        assert!(has(&s, "sf_ocr_fix"));
        assert!(!has(&s, "asr"));
        assert!(!has(&s, "tts"));
        assert!(!has(&s, "mix_audio"));
        assert!(has(&s, "mix_video"));
    }

    #[test]
    fn subtitle_asr_ocr_uses_ocr_not_dubbed() {
        // subtitle + asr_ocr: asr 提时序 + ocr 校正文本, 不配音。
        let c = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"subtitleSource": "asr_ocr"}}
            }),
        );
        let s = get_steps(&c);
        assert!(has(&s, "asr"));
        assert!(has(&s, "asr_ocr"));
        assert!(has(&s, "asr_ocr_fix"));
        assert!(!has(&s, "tts"));
        assert!(!has(&s, "mix_audio"));
        assert!(!has(&s, "sf_ocr"));
    }

    #[test]
    fn subtitle_default_is_subtitle_steps() {
        // subtitle 无 subtitleSource: 默认 asr + 仅字幕序列 (无 tts/mix_audio)。
        let c = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let s = get_steps(&c);
        assert!(has(&s, "asr"));
        assert!(has(&s, "mix_video"));
        assert!(!has(&s, "tts"));
        assert!(!has(&s, "mix_audio"));
    }
}
