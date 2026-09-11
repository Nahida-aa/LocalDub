//! 支持的语言列表 (镜像 `packages/core/const/lang.ts` 的 langList)。
//! 作为中立的语言领域常量，供 workflows / stages 共享，避免各模块重复定义。

use serde::{Deserialize, Serialize};

/// 支持的语言码列表，与 TS 侧 langList 保持同步。
pub const LANGS: &[&str] = &[
    "en", "zh", "vi", "ja", "ko", "fr", "de", "es", "pt", "ru", "ar", "hi", "th", "id", "ms", "tl",
    "my", "km", "lo", "mn", "ne", "ur", "bn",
];

/// 目标语言 (langList)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TargetLang {
    En,
    Zh,
    Vi,
    Ja,
    Ko,
    Fr,
    De,
    Es,
    Pt,
    Ru,
    Ar,
    Hi,
    Th,
    Id,
    Ms,
    Tl,
    My,
    Km,
    Lo,
    Mn,
    Ne,
    Ur,
    Bn,
}

/// 语言标识基础类型: **内部是开放字符串 (事实), 对外提供已知语言视图 (枚举)**。
///
/// 源语言是"事实" (来自 ASR 识别或用户声明), 值域开放 —— whisper 支持约
/// 99 种语言, 超出支持翻译的 23 种 [`TargetLang`] 列表; 若用封闭枚举建模,
/// 列表外语言 (it/uk/nl/...) 会被截断丢信息。用 newtype 包字符串:
/// - serde 透明 (序列化为 "it" 这样的字符串), 任何语言码原样进出
/// - [`Language::as_known`] 提供封闭 [`TargetLang`] 视图 (目标语言是
///   "承诺", 必须落在支持翻译的语言内, 保持封闭)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct Language(String);

impl Language {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// 语言码 (与 serde 名一致: "zh" / "en" / "ja" / 列表外的 "it" ...)
    pub fn code(&self) -> &str {
        &self.0
    }

    /// 已知语言视图: 语言码在支持翻译的列表内时给出 [`TargetLang`]。
    /// 列表外语言返回 None —— 事实原样保留, 但它不是可用的翻译目标。
    pub fn as_known(&self) -> Option<TargetLang> {
        TargetLang::from_code(&self.0)
    }
}

impl From<TargetLang> for Language {
    fn from(t: TargetLang) -> Self {
        Self(t.as_str().to_string())
    }
}

impl From<&str> for Language {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Language {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 兜底语言声明 (源语言未知时)。String::new 非 const, 用函数。
pub fn default_lang() -> Language {
    Language(String::new())
}

/// auto 推断目标语言: 源 zh -> en, 其它 -> zh (只看"是否 zh")。
/// 源语言是开放事实, 不受翻译目标列表约束。
pub fn infer_target_lang(src: &Language) -> TargetLang {
    if src.code() == "zh" {
        TargetLang::En
    } else {
        TargetLang::Zh
    }
}

impl TargetLang {
    /// 语言码 (与 serde 名一致: "zh" / "en" / "ja" ...)。
    /// 替代代码里散落的裸字符串字面量。
    pub fn as_str(self) -> &'static str {
        match self {
            TargetLang::En => "en",
            TargetLang::Zh => "zh",
            TargetLang::Vi => "vi",
            TargetLang::Ja => "ja",
            TargetLang::Ko => "ko",
            TargetLang::Fr => "fr",
            TargetLang::De => "de",
            TargetLang::Es => "es",
            TargetLang::Pt => "pt",
            TargetLang::Ru => "ru",
            TargetLang::Ar => "ar",
            TargetLang::Hi => "hi",
            TargetLang::Th => "th",
            TargetLang::Id => "id",
            TargetLang::Ms => "ms",
            TargetLang::Tl => "tl",
            TargetLang::My => "my",
            TargetLang::Km => "km",
            TargetLang::Lo => "lo",
            TargetLang::Mn => "mn",
            TargetLang::Ne => "ne",
            TargetLang::Ur => "ur",
            TargetLang::Bn => "bn",
        }
    }

    /// 从语言码解析 (未知码返回 None, 供 ASR 输出等运行时字符串容错)。
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(TargetLang::En),
            "zh" => Some(TargetLang::Zh),
            "vi" => Some(TargetLang::Vi),
            "ja" => Some(TargetLang::Ja),
            "ko" => Some(TargetLang::Ko),
            "fr" => Some(TargetLang::Fr),
            "de" => Some(TargetLang::De),
            "es" => Some(TargetLang::Es),
            "pt" => Some(TargetLang::Pt),
            "ru" => Some(TargetLang::Ru),
            "ar" => Some(TargetLang::Ar),
            "hi" => Some(TargetLang::Hi),
            "th" => Some(TargetLang::Th),
            "id" => Some(TargetLang::Id),
            "ms" => Some(TargetLang::Ms),
            "tl" => Some(TargetLang::Tl),
            "my" => Some(TargetLang::My),
            "km" => Some(TargetLang::Km),
            "lo" => Some(TargetLang::Lo),
            "mn" => Some(TargetLang::Mn),
            "ne" => Some(TargetLang::Ne),
            "ur" => Some(TargetLang::Ur),
            "bn" => Some(TargetLang::Bn),
            _ => None,
        }
    }
}

impl std::fmt::Display for TargetLang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 兜底语言 (源语言未知 / 目标语言 auto 推断失败时用 zh)
pub const DEFAULT_LANG: TargetLang = TargetLang::Zh;


