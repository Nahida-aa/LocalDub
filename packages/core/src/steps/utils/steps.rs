//! pipeline 阶段序列定义与解析 (镜像 TS `packages/core/steps/utils/steps.ts`)
//!
//! TS 侧 `getSteps` 通过 `readInputArgs()` 读取 subtitleSource / translate.enabled /
//! split_audio.vadAlign; Rust 侧没有该全局 singleton, 改为从 [`crate::context::WorkflowState`]
//! 的 `input` (已是 JSON Value) 解析相同字段。

use crate::state::WorkflowState;
use crate::workflows::args::{StepName, SubtitleSource};

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

/// 两维度(OutputMode × SubtitleSource)→ step 序列常量。**单一真源**:
/// 运行时 `get_steps` 与 import 期 `download::get_steps` 都走这里,
/// 避免 `subtitle + sf_ocr / asr_ocr` 这类组合在两份实现间分叉
/// (曾有一次: import 副本无条件把 tts/mix_audio 塞进序列, 而运行时序列不含)。
///
/// `is_subtitle` 直接传布尔, 以匹配 `get_steps` 原先用 `ctx.output_mode == "subtitle"`
/// 的宽松语义——`OutputMode::from_state` 对未知值 fail-fast, 这里不引入该差异。
///
/// `translate` / `split_audio` **恒在序列**(不再按开关剔除), 见 `get_steps` 下方说明;
/// 非必要开销由步骤内部自行跳过(如 `is_dub == false` 不切人声块), 不在序列层面裁剪。
pub fn steps_for(is_subtitle: bool, source: SubtitleSource) -> &'static [&'static str] {
    if is_subtitle {
        // subtitle 模式: 按 subtitleSource 切换字幕提取策略, 但始终只到 mix_video
        // (不配音 → 无 tts / mix_audio)。subtitleSource 语义是"字幕怎么提取",
        // 不是"是否配音"; dub 序列常量含 tts/mix_audio, 不能复用。
        match source {
            SubtitleSource::SfOcr => SUBTITLE_SF_OCR_STEPS,
            SubtitleSource::AsrOcr => SUBTITLE_ASR_OCR_STEPS,
            SubtitleSource::Asr => SUBTITLE_STEPS,
        }
    } else {
        // dub 模式下按 subtitleSource 选基础序列
        match source {
            SubtitleSource::SfOcr => DUB_SF_OCR_STEPS,
            SubtitleSource::AsrOcr => DUB_ASR_OCR_STEPS,
            SubtitleSource::Asr => DUB_STEPS,
        }
    }
}

/// 根据 output_mode 与 subtitleSource 过滤, 返回本次要执行的 step 序列 (镜像 TS `getSteps`)。
///
/// 返回**已解析的 [`StepName`]**：序列常量仍是字符串（便于和 TS 逐字对照），
/// 但在这里立刻解析——**写错名字会 panic**，而不是流到后面被静默跳过。
/// 有 `all_pipeline_constants_are_valid_step_names` 测试兜底，所以 panic 只在
/// 开发期触发。
///
/// **subtitleSource 走 [`SubtitleSource::from_state`]**：与 translate / mix_video
/// 同源。此前这里是独立的一份字符串解析、未知值静默默认 asr，与 translate 的
/// fail-fast 相矛盾——同一个非法值在两条路径上表现不同（一个报错、一个悄悄降级）。
/// 现在未知值统一在入口报错。
///
/// `translate` **恒在序列**（不再按 enabled 剔除）：`enabled=false` 时它以
/// **直通模式**执行（原文作为译文写 translation.json），下游步骤从此只认
/// 一种字幕来源。`translate_enabled` 仍由 step 内部读取决定走 LLM 还是直通。
/// `split_audio` **恒在序列**（不再按 `vad_align` 剔除）：它是「意图时序」
/// `split_audio/timings.json` 的唯一写入方，而下游（含 subtitle 的字幕轨）
/// 只认这一个字幕来源——剔掉它就没有时序可读。
pub fn get_steps(ctx: &WorkflowState) -> anyhow::Result<Vec<StepName>> {
    let is_subtitle = ctx.output_mode == "subtitle";
    let source = SubtitleSource::from_state(ctx)?;
    let steps = steps_for(is_subtitle, source);

    // 解析成枚举——序列常量与 `StepName` 必须一致，不一致说明有一处改名漏了。
    Ok(steps
        .iter()
        .map(|s| {
            s.parse::<StepName>().unwrap_or_else(|e| {
                panic!("序列里的 step \"{s}\" 不是合法的 StepName（{e}；改名时漏了某处？）")
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::read_ctx_from_value;
    use serde_json::json;
    use strum::IntoEnumIterator;

    fn ctx(pipeline: &str, input: serde_json::Value) -> WorkflowState {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.video_dir = "/x".into();
        ctx.output_mode = pipeline.into();
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

    /// 测试用：`get_steps` 在合法输入下必须 Ok，unwrap 掉 Result 噪音。
    fn steps_of(c: &WorkflowState) -> Vec<StepName> {
        get_steps(c).expect("合法 ctx 不应让 get_steps 失败")
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
                    s.parse::<StepName>().is_ok(),
                    "{label} 里的 \"{s}\" 不是合法的 StepName（改名时漏了某处？）"
                );
            }
        }
    }

    /// `STEPS_LIST` 与 `ALL_STEPS` 必须覆盖同一批 step（两个名单不许漂移）。
    #[test]
    fn steps_list_matches_all_steps() {
        let mut from_list: Vec<&str> = STEPS_LIST.to_vec();
        let mut from_enum = names(StepName::iter().collect::<Vec<_>>().as_slice());
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

        let mut expected: Vec<String> = StepName::iter().map(|s| s.to_string()).collect();
        expected.sort();
        let mut got = documented.clone();
        got.sort();

        assert_eq!(
            got,
            expected,
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
            names(&steps_of(&c)),
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
        assert!(has(&steps_of(&c), "sf_ocr"));
        assert!(!has(&steps_of(&c), "asr"));
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
        let s = steps_of(&c);
        assert!(has(&s, "asr_ocr"));
        assert!(has(&s, "asr_ocr_fix"));
    }

    /// translate **恒在序列**：`enabled=false` 不再剔除——step 内部走直通
    /// （原文作为译文），下游从此只认 translation.json 一种字幕来源。
    #[test]
    fn translate_disabled_keeps_step_for_passthrough() {
        let c = ctx(
            "dub",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"translate": {"enabled": false}}}
            }),
        );
        assert!(has(&steps_of(&c), "translate"));
    }

    /// **fail-fast**：未知 subtitleSource 必须在 `Input` 反序列化阶段被拒，不能静默降级成 asr。
    ///
    /// `ctx.input` 已是结构化 `Input`；未知来源不可能走到 `get_steps` /
    /// `SubtitleSource::from_state`（那些路径只会一致地回退默认 `Asr`，不会出现测试注释里
    /// 担心的「跑哪些 step」与「读哪个字幕文件」来源错乱——两者会一起回退）。fail-fast
    /// 已上移到 `Input` 解析（CLI/RPC 请求阶段，见 `SubtitleSource::from_state` 文档），
    /// 且 `read_ctx_from_value` 对该错误是 lenient 兜底（保证旧/TS state.json 不崩）。
    /// 故此处断言**类型契约**：`Input` 反序列化直接拒绝非法值。注意混合写法 `sf-OCR` 也须被拒。
    #[test]
    fn unknown_subtitle_source_is_rejected() {
        for bad in ["no_such_source", "sf-OCR", "sfocr", "asr_OCR"] {
            let err = serde_json::from_value::<crate::input::Input>(json!({
                "workflow": {"subtitleSource": bad}
            }))
            .expect_err(&format!("\"{bad}\" 应被 Input 反序列化拒绝"));
            let msg = err.to_string();
            assert!(msg.contains(bad), "错误信息应点名非法值 {bad}: {msg}");
            assert!(
                msg.contains("asr") && msg.contains("sf_ocr") && msg.contains("asr_ocr"),
                "错误信息应列出全部合法值（从 VARIANTS 生成）: {msg}"
            );
        }
    }

    /// 缺省（字段缺失）仍按 `#[default]` 走 asr —— fail-fast 只针对**非法值**，
    /// 不是把「没写」也当成错误。
    #[test]
    fn missing_subtitle_source_defaults_to_asr() {
        let c = ctx(
            "dub",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert!(has(&steps_of(&c), "asr"));
        assert!(!has(&steps_of(&c), "sf_ocr"));
    }

    /// split_audio 恒在序列（含 `vad_align=false`）：它是 `timings.json` 的唯一
    /// 写入方，下游只认这一个字幕来源。非必要开销由步骤内部按 `is_dub` /
    /// `vad_align` 自行跳过，不再靠序列裁剪。
    #[test]
    fn subtitle_always_includes_split_audio() {
        let c = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        assert!(has(&steps_of(&c), "split_audio"));

        let c2 = ctx(
            "subtitle",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"split_audio": {"vadAlign": true}}}
            }),
        );
        assert!(has(&steps_of(&c2), "split_audio"));
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
        let s = steps_of(&c);
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
        let s = steps_of(&c);
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
        let s = steps_of(&c);
        assert!(has(&s, "asr"));
        assert!(has(&s, "mix_video"));
        assert!(!has(&s, "tts"));
        assert!(!has(&s, "mix_audio"));
    }
}
