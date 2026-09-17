//! 语言对象的**状态投影**：从 [`WorkflowState`] 解析出「源语言」与「目标语言」两个一等对象。
//!
//! 把"从 state 取语言"这件事从各 step 内联逻辑提升为显式 in-edge，契合代码即 DAG：
//! step 声明它要从 state 解析哪个语言，而不是在 step 内部去戳 `asr_language` 等字段。
//!
//! 解析策略（`resolve_source_language` / `resolve_target_language`）与兜底/推断逻辑
//! （`default_lang` / `infer_target_lang`）就放在本模块、紧贴使用它们的对象——"语言如何
//! 从 state 推导"的故事在一个文件里讲完。纯类型模型（[`LangCode`] / [`Language`] 枚举）
//! 留在 `const::lang`（叶子模块，不依赖 state）。

use crate::r#const::lang::{Language, LangCode};
use crate::state::WorkflowState;

/// 源语言（解析后的投影对象）。
///
/// 解析链: `asr_language`（ASR 实测）> `input.workflow.sourceLang` > 兜底（未知/空串）。
/// 纯 OCR / subtitle 路径没有 ASR，`asr_language` 为 None，自动回落到配置的 sourceLang，
/// 否则透传空串（见 [`crate::r#const::lang::default_lang`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLang(pub LangCode);

impl SourceLang {
    pub fn from_state(s: &WorkflowState) -> Self {
        Self(resolve_source_language(
            s.asr_language.as_ref(),
            s.input
                .workflow
                .as_ref()
                .and_then(|w| w.source_lang.as_ref()),
        ))
    }

    pub fn code(&self) -> &str {
        self.0.code()
    }

    pub fn into_inner(self) -> LangCode {
        self.0
    }
}

/// 目标语言（解析后的投影对象）。
///
/// 解析链: `input.workflow.targetLang`（显式）> 按源语言 auto 推断（源 zh -> en，其它 -> zh）。
///
/// 注意: 当前与 `steps::utils::resolve_language` 保持一致——只读取配置级 `targetLang`，
/// 不改写也不读取已落盘的 `ctx.target_language`（那是 translate 步骤写入的事实，由
/// `TranslateOutput::from_ctx` 单独读取）。若未来要把"目标语言"统一走强落盘事实，
/// 应在此处改为优先读 `s.target_language`，而非在 step 内各自现算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetLang(pub Language);

impl TargetLang {
    pub fn from_state(s: &WorkflowState) -> Self {
        let src = SourceLang::from_state(s);
        let explicit = s
            .input
            .workflow
            .as_ref()
            .and_then(|w| w.target_lang);
        Self(resolve_target_language(&src.0, explicit))
    }

    pub fn code(self) -> &'static str {
        self.0.into()
    }

    pub fn into_inner(self) -> Language {
        self.0
    }
}

/// 解析**源语言**（状态投影策略）：ASR 实测优先，回落到配置 `source_lang`，
/// 再回落到 [`default_lang`]。紧贴 [`SourceLang`] 放在本模块，使"源语言如何从 state
/// 推导"的故事集中在一处。
pub(crate) fn resolve_source_language(
    asr: Option<&LangCode>,
    source: Option<&LangCode>,
) -> LangCode {
    asr.cloned()
        .or_else(|| source.cloned())
        .unwrap_or_else(default_lang)
}

/// 解析**目标语言**（状态投影策略）：显式目标优先，否则按源语言 [`infer_target_lang`]
/// auto 推断。紧贴 [`TargetLang`] 放在本模块。
pub(crate) fn resolve_target_language(src: &LangCode, explicit: Option<Language>) -> Language {
    explicit.unwrap_or_else(|| infer_target_lang(src))
}

/// 兜底源语言（源语言未知时）。返回 [`LangCode::unknown`]（空串）；下游透传语言码，
/// 切勿臆测成 zh。此前的 "默认 zh" 注释是误导：强制 zh 会把外语原文误修正为中文。
///
/// 与 [`infer_target_lang`] 同属状态投影的兜底/推断逻辑，故放在本模块而非
/// `const::lang`（纯类型模型）。
pub(crate) fn default_lang() -> LangCode {
    LangCode::unknown()
}

/// auto 推断目标语言: 源 zh -> en, 其它 -> zh（只看"是否 zh"）。
/// 源语言是开放事实, 不受翻译目标列表约束。
///
/// 与 [`default_lang`] 同属状态投影的兜底/推断逻辑，紧贴 [`resolve_target_language`]
/// 放在本模块；纯类型模型留在 `const::lang`。
pub(crate) fn infer_target_lang(src: &LangCode) -> Language {
    if src.code() == "zh" {
        Language::En
    } else {
        Language::Zh
    }
}
