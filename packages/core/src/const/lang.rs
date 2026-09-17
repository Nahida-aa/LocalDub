//! 支持的语言列表 (镜像 `packages/core/const/lang.ts` 的 langList)。
//! 作为中立的语言领域常量，供 workflows / steps 共享，避免各模块重复定义。

use serde::{Deserialize, Serialize};

/// 语言码与变体的**单一事实来源**。闭环的 [`Language`] 枚举、`as_str`、`from_code`、
/// 以及 [`LANGS`] 常量都从这里展开，避免同一份 23 语言被列三遍（枚举、as_str、
/// from_code 各一遍）导致加语言时要改三处。
macro_rules! lang_variants {
    ($($variant:ident => $code:literal),+ $(,)?) => {
        /// 目标语言（支持翻译的封闭集合，与 TS 侧 langList 保持同步）。
        ///
        /// 与源语言 [`LangCode`] 不同：目标是"承诺"——必须落在支持翻译的语言内，
        /// 故用封闭枚举建模；列表外语言不是可用的翻译目标。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
        #[serde(rename_all = "lowercase")]
        pub enum Language {
            $($variant),+
        }

        impl Language {
            /// 语言码（与 serde 名一致: "zh" / "en" / "ja" ...）。
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Language::$variant => $code),+
                }
            }
            /// 从语言码解析（未知码返回 None，供 ASR 输出等运行时字符串容错）。
            pub fn from_code(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Language::$variant)),+,
                    _ => None,
                }
            }
        }

        /// 支持翻译的语言码列表，与 TS 侧 langList 保持同步。
        pub const LANGS: &[&str] = &[$($code),+];
    };
}

lang_variants! {
    En => "en", Zh => "zh", Vi => "vi", Ja => "ja", Ko => "ko", Fr => "fr",
    De => "de", Es => "es", Pt => "pt", Ru => "ru", Ar => "ar", Hi => "hi",
    Th => "th", Id => "id", Ms => "ms", Tl => "tl", My => "my", Km => "km",
    Lo => "lo", Mn => "mn", Ne => "ne", Ur => "ur", Bn => "bn",
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
/// - [`LangCode::as_known`] 提供封闭 [`Language`] 视图（目标语言是
///   "承诺"，必须落在支持翻译的语言内，保持封闭）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct LangCode(String);

impl LangCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// 语言码（与 serde 名一致: "zh" / "en" / "ja" / 列表外的 "it" ...）
    pub fn code(&self) -> &str {
        &self.0
    }

    /// 已知语言视图: 语言码在支持翻译的列表内时给出 [`Language`]。
    /// 列表外语言返回 None——事实原样保留，但它不是可用的翻译目标。
    pub fn as_known(&self) -> Option<Language> {
        Language::from_code(&self.0)
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

impl std::fmt::Display for LangCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 兜底源语言（源语言未知时）。返回 [`LangCode::unknown`]（空串）；下游透传语言码，
/// 切勿臆测成 zh。此前的 "默认 zh" 注释是误导：强制 zh 会把外语原文误修正为中文。
pub fn default_lang() -> LangCode {
    LangCode::unknown()
}

/// auto 推断目标语言: 源 zh -> en, 其它 -> zh（只看"是否 zh"）。
/// 源语言是开放事实, 不受翻译目标列表约束。
pub fn infer_target_lang(src: &LangCode) -> Language {
    if src.code() == "zh" {
        Language::En
    } else {
        Language::Zh
    }
}

/// 纯函数解析**源语言**（不依赖 `WorkflowState`）：ASR 实测优先，回落到配置
/// `source_lang`，再回落到 [`default_lang`]。供 `state::lang::SourceLang::from_state` 调用。
pub fn resolve_source_language(
    asr: Option<&LangCode>,
    source: Option<&LangCode>,
) -> LangCode {
    asr.cloned()
        .or_else(|| source.cloned())
        .unwrap_or_else(default_lang)
}

/// 纯函数解析**目标语言**（不依赖 `WorkflowState`）：显式目标优先，否则按源语言
/// auto 推断。供 `state::lang::TargetLang::from_state` 调用。
pub fn resolve_target_language(src: &LangCode, explicit: Option<Language>) -> Language {
    explicit.unwrap_or_else(|| infer_target_lang(src))
}
