use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

/// translate 阶段参数 (镜像 TS `packages/core/steps/05_translate/args.ts` TranslateArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 目标语言统一在 `input.workflow.targetLang` 配置 (任务级概念: 一个任务只有一个翻译
/// 阶段, 不需要 step 级覆盖)。旧的 `steps.translate.targetLang` 已移除,
/// deny_unknown_fields 让残留配置明确报错 (不能静默失效 -> 翻错语言)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranslateArgs {
    /// OpenAI 兼容端点
    #[serde(default = "openai_base_url")]
    pub api_base: String,
    /// 翻译模型
    #[serde(default = "openai_model")]
    pub model: String,
    /// 设为 false 跳过翻译, 直接使用原始识别文本
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for TranslateArgs {
    /// **必须手写**：派生的 `Default` 会把 `bool` 置为 `false`，与 serde 的
    /// `default = "default_true"` 相反。曾导致 `unwrap_or_default()`（配置对象
    /// 整个缺失时）得到 enabled=false，而「对象存在但缺字段」得到 true——
    /// 两条路径语义相反。
    fn default() -> Self {
        Self {
            api_base: openai_base_url(),
            model: openai_model(),
            enabled: true,
        }
    }
}

fn default_true() -> bool {
    true
}
