//! 语言对象的**状态投影**：从 [`WorkflowState`] 解析出「源语言」与「目标语言」两个一等对象。
//!
//! 把"从 state 取语言"这件事从各 step 内联逻辑提升为显式 in-edge，契合代码即 DAG：
//! step 声明它要从 state 解析哪个语言，而不是在 step 内部去戳 `asr_language` 等字段。
//!
//! 纯解析逻辑放在 `const::lang`（`resolve_source_language` / `resolve_target_language`），
//! 本模块只负责把 `WorkflowState` 的事实喂进去——保持 `const::lang` 仍是叶子模块。

use crate::r#const::lang::{resolve_source_language, resolve_target_language, Language, LangCode};
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
        self.0.as_str()
    }

    pub fn into_inner(self) -> Language {
        self.0
    }
}
