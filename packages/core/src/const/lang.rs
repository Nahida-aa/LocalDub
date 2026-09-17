//! 支持的语言列表 (镜像 `packages/core/const/lang.ts` 的 langList)。
//! 作为中立的语言领域常量，供 workflows / steps 共享，避免各模块重复定义。

use serde::{Deserialize, Serialize};

/// 目标语言（支持翻译的封闭集合，与 TS 侧 langList 保持同步）。
///
/// 与源语言 [`LangCode`] 不同：目标是"承诺"——必须落在支持翻译的语言内，
/// 故用封闭枚举建模；列表外语言不是可用的翻译目标。
///
/// 变体名即语言码的小写形式（`En`→`"en"`）。serde 的 `rename_all` 与 strum 的
/// `serialize_all` 都按 "lowercase" 从变体名导出码，单一事实来源仍是变体列表本身——
/// 加语言只改这一处。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type,
    strum::IntoStaticStr, strum::Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Language {
    En, Zh, Vi, Ja, Ko, Fr,
    De, Es, Pt, Ru, Ar, Hi,
    Th, Id, Ms, Tl, My, Km,
    Lo, Mn, Ne, Ur, Bn,
}

impl Language {
    /// 语言码（与 serde 名一致: "zh" / "en" / "ja" ...）。
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// 兜底目标语言（目标语言 auto 推断失败 / 未配置时用 zh）。
pub const DEFAULT_LANG: Language = Language::Zh;

/// 源语言标识基础类型: **内部是开放字符串（事实）**。
///
/// 源语言来自 ASR 识别或用户声明，值域开放——whisper 支持约 99 种语言，
/// 超出支持翻译的 [`Language`] 列表；若用封闭枚举建模，列表外语言
/// (it/uk/nl/...) 会被截断丢信息。用 newtype 包字符串:
/// - serde 透明（序列化为 "it" 这样的字符串），任何语言码原样进出
/// - 封闭视图: 需要"支持翻译的目标语言"时改用 [`Language`] 枚举（"承诺"，必须落在
///   支持翻译的语言内；列表外语言不是可用的翻译目标）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type, derive_more::Display)]
pub struct LangCode(String);

impl LangCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// 语言码（与 serde 名一致: "zh" / "en" / "ja" / 列表外的 "it" ...）
    pub fn code(&self) -> &str {
        &self.0
    }

    /// 未知源语言（未设置）。序列化出去是空串，调用方应据此**透传**语言码，
    /// 而非臆测成 zh——否则会把外语原文误导成中文修正（见 `llm::lang_label`
    /// 的设计说明）。
    pub fn unknown() -> Self {
        LangCode(String::new())
    }
}

impl From<Language> for LangCode {
    fn from(t: Language) -> Self {
        Self(t.as_str().to_string())
    }
}

impl From<&str> for LangCode {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for LangCode {
    fn from(s: String) -> Self {
        Self(s)
    }
}
