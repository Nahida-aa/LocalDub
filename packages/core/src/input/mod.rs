//! 顶层输入类型 (不限定 CLI 场景, Tauri RPC / pipeline 均可复用)。
//!
//! 镜像 TS 侧 `packages/core/input/types.ts`：
//! - `task` args → [`tasks::args`](crate::tasks::args)
//! - 各 pipeline 阶段参数 → [`stages`](stages)
//!
//! input 语义由 `specta_serde::PhasesFormat` 驱动：`#[serde(default)]` 的字段在
//! Deserialize 面（input）可选、在 Serialize 面（output）必填，对齐 zod 的 io 区分。

use serde::{Deserialize, Serialize};

use crate::servers::args::ServersArgs;
use crate::tasks::args;

pub mod stages;

/// 命令
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Command {
    Task,
    Env,
    Servers,
}

impl Default for Command {
    fn default() -> Self {
        Self::Env
    }
}

/// 顶层输入
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Input {
    /// 任务参数, 仅 command=task 时必须
    pub task: Option<args::TaskArgs>,
    /// 执行命令 (默认 env)
    #[serde(default)]
    pub command: Command,
    /// 服务端参数 (镜像 servers/args.ts), 仅 command=servers 时使用
    #[serde(default)]
    pub servers: Option<ServersArgs>,
    #[serde(default)]
    pub stages: stages::Stages,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            task: None,
            command: Command::default(),
            servers: None,
            stages: stages::Stages::default(),
        }
    }
}

impl Input {
    /// command=task 时 task 必填
    ///
    /// 目前主要被测试引用；CLI/RPC 解析入口可直接调用。
    pub fn validate(&self) -> Result<(), String> {
        if self.command == Command::Task && self.task.is_none() {
            return Err("command=task 时 task 必填".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_partial_fills_defaults() {
        let input: Input =
            serde_json::from_str(r#"{"command":"env","task":{"pipeline":"subtitle"}}"#).unwrap();
        assert_eq!(input.command, Command::Env);
        assert!(input.stages.asr.mix_mode == stages::MixMode::Sidechain);
        assert_eq!(input.stages.asr.reduce_bgm, -12.0);
        assert_eq!(
            input.task.as_ref().unwrap().pipeline,
            args::Pipeline::Subtitle
        );
        assert_eq!(
            input.task.as_ref().unwrap().subtitle_source,
            args::SubtitleSource::Asr
        );
    }

    #[test]
    fn camel_case_field_names() {
        let input: Input =
            serde_json::from_str(r#"{"task":{"sourceLang":"zh","targetStage":"mix_video"}}"#)
                .unwrap();
        assert_eq!(
            input.task.as_ref().unwrap().source_lang,
            Some(crate::r#const::lang::TargetLang::Zh)
        );
        assert_eq!(
            input.task.as_ref().unwrap().target_stage,
            Some(args::StageName::MixVideo)
        );
    }

    #[test]
    fn validate_task_required_for_task_command() {
        let ok: Input = serde_json::from_str(r#"{"command":"task","task":{}}"#).unwrap();
        assert!(ok.validate().is_ok());

        let missing: Input = serde_json::from_str(r#"{"command":"task"}"#).unwrap();
        assert!(missing.validate().is_err());

        let env: Input = serde_json::from_str(r#"{"command":"env"}"#).unwrap();
        assert!(env.validate().is_ok());
    }

    #[test]
    fn servers_field_wires_servers_args() {
        let input: Input = serde_json::from_str(
            r#"{"command":"servers","servers":{"action":"stop","name":"voxcpm_torch_gradio"}}"#,
        )
        .unwrap();
        assert_eq!(input.command, Command::Servers);
        let servers = input.servers.unwrap();
        assert_eq!(servers.action, crate::servers::args::ServerAction::Stop);
        assert!(matches!(
            servers.name,
            Some(config_rs::servers::ServerType::VoxcpmTorchGradio)
        ));

        let empty: Input = serde_json::from_str(r#"{"command":"servers"}"#).unwrap();
        assert!(empty.servers.is_none());
    }
}
