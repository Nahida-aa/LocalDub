//! 环境检测命令 (`cli env [check|ensure] [targets...]`)。
//!
//! 镜像 TS `packages/core/cmd/env/index.ts`: `runCheck` / `runEnsure` /
//! `formatResult` + `resolveTargets`。检查逻辑见 `items.rs`, 元信息见 `input.rs`。

pub mod input;
pub mod items;

use serde::Serialize;

use crate::cmd::env::input::{env_names, zh_desc};
use crate::cmd::env::items::{all_checks, ensure_fns};

/// 检查状态 (serde 小写对齐 TS 字符串 "pass"/"warn"/"fail"/"skip")。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

/// 单项检查结果 (对齐 TS `CheckResult`)。
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub key: String,
    pub status: CheckStatus,
    /// 容纳 version/size/issues/missing_bins/stale_bins/path 等自由字段。
    pub data: serde_json::Value,
    pub required: bool,
}

/// 全部环境项 key 集合 (镜像 TS `envList`)。
pub fn env_list() -> Vec<&'static str> {
    env_names()
}

/// 解析目标: 空 → 全部; 过滤到 all_checks 中存在的 key; 过滤后空 → 全部。
fn resolve_targets(targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return env_names().iter().map(|s| s.to_string()).collect();
    }
    let checks = all_checks();
    let valid: Vec<String> = targets
        .iter()
        .filter(|t| checks.contains_key(t.as_str()))
        .cloned()
        .collect();
    if valid.is_empty() {
        return env_names().iter().map(|s| s.to_string()).collect();
    }
    valid
}

/// 运行检查 (镜像 TS `runCheck`)。
pub fn run_check(targets: &[String]) -> Vec<CheckResult> {
    let selected = resolve_targets(targets);
    let checks = all_checks();
    let mut results = Vec::new();
    for key in &selected {
        match checks.get(key.as_str()) {
            Some(f) => {
                // 单 check 内部已吞异常式处理; 这里再包一层防御
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(
                    |_| CheckResult {
                        key: key.clone(),
                        status: CheckStatus::Fail,
                        data: serde_json::json!({ "msg": "检查过程 panic" }),
                        required: false,
                    },
                );
                results.push(r);
            }
            None => results.push(CheckResult {
                key: key.clone(),
                status: CheckStatus::Skip,
                data: serde_json::json!({}),
                required: false,
            }),
        }
    }
    results
}

/// 运行 ensure (镜像 TS `runEnsure`)。
pub fn run_ensure(targets: &[String]) -> Vec<CheckResult> {
    let selected = resolve_targets(targets);
    let fns = ensure_fns();
    let mut results = Vec::new();
    for key in &selected {
        match fns.get(key.as_str()) {
            Some(f) => {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(
                    |_| CheckResult {
                        key: key.clone(),
                        status: CheckStatus::Fail,
                        data: serde_json::json!({ "msg": "ensure 过程 panic" }),
                        required: false,
                    },
                );
                results.push(r);
            }
            None => results.push(CheckResult {
                key: key.clone(),
                status: CheckStatus::Skip,
                data: serde_json::json!({}),
                required: false,
            }),
        }
    }
    results
}

/// 从 data 提取简化 msg (优先 `msg`, 否则拼接关键字段)。
fn extract_msg(r: &CheckResult) -> String {
    if let Some(m) = r.data.get("msg").and_then(|v| v.as_str()) {
        if !m.is_empty() {
            return m.to_string();
        }
    }
    // 退化: 拼接 size / version / issues / found/total
    let mut parts = Vec::new();
    for k in ["size", "version", "issues", "missing", "gpu"] {
        if let Some(v) = r.data.get(k) {
            parts.push(v.to_string());
        }
    }
    if let (Some(f), Some(t)) = (r.data.get("found"), r.data.get("total")) {
        parts.push(format!("{f}/{t}"));
    }
    parts.join(" ").trim().to_string()
}

/// 格式化单行结果 (镜像 TS `formatResult`)。
pub fn format_result(r: &CheckResult) -> String {
    let prefix = match r.status {
        CheckStatus::Pass => "  ✓",
        CheckStatus::Warn => "  ⚠",
        CheckStatus::Fail | CheckStatus::Skip => "  ✗",
    };
    let msg = extract_msg(r);
    let first = format!("{prefix} {} — {}  ({:?})", r.key, msg, r.status);

    let mut extras: Vec<String> = Vec::new();
    if let Some(m) = r.data.get("missing_bins").and_then(|v| v.as_str()) {
        if !m.is_empty() {
            extras.push(format!("    missing: {m}"));
        }
    }
    if let Some(s) = r.data.get("stale_bins").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            extras.push(format!("    stale: {s}"));
        }
    }
    if let Some(f) = r.data.get("fresh_bins").and_then(|v| v.as_str()) {
        if !f.is_empty() {
            extras.push(format!("    fresh: {f}"));
        }
    }

    let desc = zh_desc(&r.key);
    let mut lines = vec![first];
    lines.extend(extras);
    if !desc.is_empty() {
        format!("{}\n  {}", lines.join("\n"), desc)
    } else {
        lines.join("\n")
    }
}
