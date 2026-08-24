//! `env` 命令参数 (镜像 TS `packages/core/cmd/env/input.ts` 的 EnvArgsSchema)。

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// `env` 命令参数。
///
/// `action` 决定 check / ensure; `targets` 指定要检查的环境项 key (空 → 按 stages 配置推断)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvArgs {
    /// 动作: check (默认) / ensure
    #[serde(default)]
    pub action: EnvAction,
    /// 要检查/ensure 的环境项; 空数组 → 按 input.jsonc 的 stages 配置推断所需项
    #[serde(default)]
    pub targets: Vec<String>,
}

/// env 动作 (也用于 clap `--action` 命令行解析)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type, ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum EnvAction {
    #[default]
    Check,
    Ensure,
}
