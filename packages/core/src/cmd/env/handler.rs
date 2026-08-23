//! `env` 命令入口 (镜像 TS `run-task.ts` 的 `command: env` 分支)。
//!
//! 纯从 `input.env` 派发: `action` (check/ensure) + `targets` (空 → 按 stages 推断)。
//! CLI 的显式 `--action`/`--targets` 覆盖逻辑保留在 cli (它调用 `cmd::env` 的
//! `run_check`/`run_ensure`/`format_result`); 这里是从 `Input` 出发的统一入口。

use crate::cmd::env::args::EnvAction;
use crate::cmd::env::{format_result, infer_targets, run_check, run_ensure};
use crate::input::Input;

/// 运行 env 命令 (镜像 TS `run-task.ts` 的 env 分支)。
///
/// - action: 取 `input.env.action` (默认 check)
/// - targets: 取 `input.env.targets`; 空 → 按 stages 配置推断 (`infer_targets`)
pub fn cmd_env(input: &Input) -> anyhow::Result<()> {
    let env_args = input.env.clone().unwrap_or_default();

    let (targets, desired) = if env_args.targets.is_empty() {
        infer_targets(input)
    } else {
        (env_args.targets, std::collections::HashMap::new())
    };

    let results = match env_args.action {
        EnvAction::Ensure => run_ensure(&targets, &desired),
        EnvAction::Check => run_check(&targets, &desired),
    };
    for r in &results {
        println!("{}", format_result(r));
    }
    Ok(())
}
