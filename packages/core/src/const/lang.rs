//! 支持的语言列表 (镜像 `packages/core/const/lang.ts` 的 langList)。
//! 作为中立的语言领域常量，供 tasks / stages 共享，避免各模块重复定义。

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

/// 源语言: **开放字符串**, 不与 TargetLang (封闭枚举) 共用类型。
///
/// 源语言是"事实" (来自 ASR 识别或用户声明), 外部世界的语言码不限于
/// 支持翻译的 23 种 (whisper 支持约 99 种) —— 封闭枚举会把列表外语言
/// 丢信息; 目标语言是"承诺", 保持封闭。

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

/// auto 推断目标语言: 源 zh -> en, 其它 -> zh (只看"是否 zh")。
/// 参数为开放字符串 (源语言是事实, 不受翻译目标列表约束)。
pub fn infer_target_lang(src_code: &str) -> TargetLang {
    if src_code == "zh" {
        TargetLang::En
    } else {
        TargetLang::Zh
    }
}
