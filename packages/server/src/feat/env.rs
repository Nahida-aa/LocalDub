use std::collections::HashMap;

use ld_core::cmd::env::input::EnvKey;
use ld_core::cmd::env::{env_list, infer_targets, run_check, run_ensure, CheckResult, CheckStatus};
use paths::parse_repo_input;
use serde::{Deserialize, Serialize};
use specta::Type;

/// 单项环境检查结果 (前端展示用 DTO, 聚合 core 的 CheckResult + 元数据)。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EnvCheckItem {
    pub key: String,
    pub zh: String,
    pub en: String,
    pub required: bool,
    pub category: String,
    /// "pass" / "warn" / "fail" / "skip"
    pub status: String,
    pub data: serde_json::Value,
    /// 是否存在对应安装动作 (EnvKey::has_ensure)。
    pub has_ensure: bool,
}

fn all_keys() -> Vec<EnvKey> {
    env_list().into_iter().filter_map(EnvKey::parse).collect()
}

fn meta_of(key: &str) -> (String, String, bool, String) {
    match EnvKey::parse(key) {
        Some(k) => {
            let e = k.meta();
            (e.zh.to_string(), e.en.to_string(), e.required, e.category.to_string())
        }
        None => (key.to_string(), key.to_string(), false, "unknown".to_string()),
    }
}

fn assemble(results: Vec<CheckResult>) -> Vec<EnvCheckItem> {
    results
        .into_iter()
        .map(|r| {
            let (zh, en, required, category) = meta_of(&r.key);
            let status = match r.status {
                CheckStatus::Pass => "pass",
                CheckStatus::Warn => "warn",
                CheckStatus::Fail => "fail",
                CheckStatus::Skip => "skip",
            };
            EnvCheckItem {
                key: r.key.clone(),
                zh,
                en,
                required,
                category,
                status: status.to_string(),
                data: r.data,
                has_ensure: EnvKey::parse(&r.key).is_some_and(|k| k.has_ensure()),
            }
        })
        .collect()
}

/// 读仓库根 input.jsonc/input.json 并反序列化为 Input (jsonc-parser 处理注释/尾逗号)。
fn repo_input() -> anyhow::Result<ld_core::input::Input> {
    parse_repo_input()
}

/// 字符串 targets → EnvKey 的边界：非法 key 直接报错，不再静默跳过。
fn parse_targets(targets: &[String]) -> Result<Vec<EnvKey>, String> {
    targets
        .iter()
        .map(|s| EnvKey::parse(s).ok_or_else(|| format!("未知环境项: {s}")))
        .collect()
}

/// 环境检查。
///
/// `targets` 空 = 按当前 input.jsonc 配置推断; 含 "*" = 全部项; 否则精确运行指定 key。
#[fnrpc::rpc_query]
pub async fn env_check(targets: Vec<String>) -> Result<Vec<EnvCheckItem>, String> {
    let (keys, desired) = if targets.is_empty() {
        match repo_input() {
            Ok(input) => infer_targets(&input),
            Err(e) => {
                tracing::info!("env_check 推断失败, 回退全量: {e:#}");
                (all_keys(), HashMap::new())
            }
        }
    } else if targets.iter().any(|t| t == "*") {
        (all_keys(), HashMap::new())
    } else {
        (parse_targets(&targets)?, HashMap::new())
    };
    Ok(assemble(run_check(&keys, &desired).await))
}

#[fnrpc::rpc_mutate]
pub async fn env_ensure(targets: Vec<String>) -> Result<Vec<EnvCheckItem>, String> {
    let (keys, desired) = if targets.is_empty() {
        match repo_input() {
            Ok(input) => infer_targets(&input),
            Err(e) => {
                tracing::info!("env_ensure 推断失败, 回退全量: {e:#}");
                (all_keys(), HashMap::new())
            }
        }
    } else {
        (parse_targets(&targets)?, HashMap::new())
    };
    Ok(assemble(run_ensure(&keys, &desired).await))
}