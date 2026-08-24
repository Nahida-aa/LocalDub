use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

use crate::r#const::lang::TargetLang;

/// translate 阶段参数 (镜像 TS `packages/core/stages/05_translate/args.ts` TranslateArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranslateArgs {
    /// OpenAI 兼容端点
    #[serde(default = "openai_base_url")]
    pub api_base: String,
    /// 翻译模型
    #[serde(default = "openai_model")]
    pub model: String,
    /// 目标语言; 不填则按逻辑: 源语言 zh -> en, 否则 any -> zh
    #[serde(default)]
    pub target_lang: Option<TargetLang>,
    /// 设为 false 跳过翻译, 直接使用原始识别文本
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}
