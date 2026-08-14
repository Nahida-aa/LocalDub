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

/// 源语言与 TargetLang 共用同一枚举
pub type SourceLang = TargetLang;
