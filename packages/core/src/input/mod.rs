//! 顶层输入类型 (不限定 CLI 场景, Tauri RPC / pipeline 均可复用)。
//!
//! 镜像 TS 侧 `packages/core/input/types.ts`：
//! - `workflow` args → [`workflow::args`](crate::workflow::args)
//! - 各 pipeline 阶段参数 → [`steps`](steps)
//!
//! input 语义由 `specta_serde::PhasesFormat` 驱动：`#[serde(default)]` 的字段在
//! Deserialize 面（input）可选、在 Serialize 面（output）必填，对齐 zod 的 io 区分。

use serde::{Deserialize, Serialize};

use crate::cmd::check::CheckArgs;
use crate::cmd::cookie::CookieArgs;
use crate::cmd::env::args::EnvArgs;
use crate::servers::args::ServersArgs;
use crate::workflow::args;

pub mod steps;

/// 命令
///
/// 命名对齐 TS `commandList` (types.ts): `deviceInfo`/`listModels` 是 camelCase,
/// 其余为全小写 (增强 `rename_all = "lowercase"` 无法表达, 显式 rename)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Command {
    Workflow,
    Env,
    Servers,
    Cookie,
    Check,
    #[serde(rename = "deviceInfo")]
    DeviceInfo,
    #[serde(rename = "listModels")]
    ListModels,
}

impl Default for Command {
    fn default() -> Self {
        Self::Env
    }
}

/// 顶层输入
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Input {
    /// workflow 参数, 仅 command=workflow 时必须
    pub workflow: Option<args::WorkflowArgs>,
    /// 执行命令 (默认 env)
    #[serde(default)]
    pub command: Command,
    /// 服务端参数 (镜像 servers/args.ts), 仅 command=servers 时使用
    #[serde(default)]
    pub servers: Option<ServersArgs>,
    /// env 命令参数 (镜像 env/input.ts); targets 为空时按 steps 配置推断所需环境项
    #[serde(default)]
    pub env: Option<EnvArgs>,
    /// cookie 命令参数 (镜像 cmd/cookie/args.ts), 仅 command=cookie 时使用
    #[serde(default)]
    pub cookie: Option<CookieArgs>,
    /// check 命令参数 (镜像 cmd/check 参数 schema), 仅 command=check 时使用
    #[serde(default)]
    pub check: Option<CheckArgs>,
    #[serde(default)]
    pub steps: steps::Steps,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            workflow: None,
            command: Command::default(),
            servers: None,
            env: None,
            cookie: None,
            check: None,
            steps: steps::Steps::default(),
        }
    }
}

impl Input {
    /// 任务目录 = `input.workflow.video_dir`。
    ///
    /// pipeline 入口 ([`crate::workflow::pipeline::run_pipeline`] /
    /// [`crate::workflow::continue_pipeline`] / [`crate::workflow::parallel::run_pipeline_parallel`])
    /// 的 video_dir 都从这里取，不再由调用方显式传参：start 路径由 import_video
    /// 身份回填保证它是落定的真实目录；continue 路径本来就必填。
    pub fn video_dir(&self) -> anyhow::Result<&str> {
        self.workflow
            .as_ref()
            .and_then(|w| w.video_dir.as_deref())
            .ok_or_else(|| anyhow::anyhow!("input.workflow.video_dir 缺失 (start 由 import_video 回填, continue 必填)"))
    }

    /// command=workflow 时 workflow 必填
    ///
    /// 目前主要被测试引用；CLI/RPC 解析入口可直接调用。
    pub fn validate(&self) -> Result<(), String> {
        if self.command == Command::Workflow && self.workflow.is_none() {
            return Err("command=workflow 时 workflow 必填".into());
        }
        Ok(())
    }
}

/// **工作流输入**——import 回填后的 run input（步骤/执行器消费的类型）。
///
/// 与 [`Input`] 的区别纯粹是**类型层面的保障**，零行为变化：
/// - `Input`（来自 `input.jsonc`）：`workflow: Option<WorkflowArgs>`、
///   `video_dir: Option<String>`——start 时用户可不填，由 import 回填；
/// - `WorkflowInput`（import 后）：`workflow: WorkflowArgs`（非 Option）、
///   `video_dir()` 返回 `&str`（不返回 `Result`）——import 是 run input 的生产者，
///   步骤拿到的必然是已落定身份的事实。
///
/// 只保留步骤真正读的两个字段（`workflow` / `steps`）；其余命令参数
/// （command/servers/env/cookie/check）属于 CLI/RPC 层，步骤从不读。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInput {
    pub workflow: args::WorkflowArgs,
    pub steps: steps::Steps,
}

impl WorkflowInput {
    /// 任务目录——由 [`TryFrom<Input>`] 保证 `workflow.video_dir` 为 Some。
    pub fn video_dir(&self) -> &str {
        self.workflow
            .video_dir
            .as_deref()
            .expect("WorkflowInput 构造时已校验 video_dir")
    }
}

impl TryFrom<Input> for WorkflowInput {
    type Error = String;

    fn try_from(input: Input) -> Result<Self, Self::Error> {
        let workflow = input
            .workflow
            .ok_or_else(|| "workflow 缺失（import_video 未产出 run input）".to_string())?;
        if workflow.video_dir.is_none() {
            return Err("workflow.video_dir 缺失（start 由 import_video 回填，continue 必填）".into());
        }
        Ok(Self {
            workflow,
            steps: input.steps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_partial_fills_defaults() {
        let input: Input =
            serde_json::from_str(r#"{"command":"env","workflow":{"output_mode":"subtitle"}}"#)
                .unwrap();
        assert_eq!(input.command, Command::Env);
        assert!(
            input.steps.separate_after.mix_mode
                == crate::steps::separate::after_args::MixMode::Sidechain
        );
        assert_eq!(input.steps.separate_after.reduce_bgm, -12.0);
        assert_eq!(
            input.workflow.as_ref().unwrap().output_mode,
            args::OutputMode::Subtitle
        );
        assert_eq!(
            input.workflow.as_ref().unwrap().subtitle_source,
            args::SubtitleSource::Asr
        );
    }

    /// **wire 键名双向钉住**：`WorkflowArgs` 是 `rename_all = "snake_case"`，
    /// `output_mode` 走默认规则（无显式 rename）。这里双向钉住：反序列化认
    /// `output_mode`，序列化也产出 `output_mode`，不冒出 camelCase 变体。
    #[test]
    fn output_mode_wire_key_is_snake_case_both_ways() {
        let input: Input =
            serde_json::from_str(r#"{"workflow":{"output_mode":"subtitle"}}"#).unwrap();
        assert_eq!(
            input.workflow.as_ref().unwrap().output_mode,
            args::OutputMode::Subtitle
        );

        // 反方向：序列化出来的键必须仍是 `output_mode`
        let v = serde_json::to_value(input.workflow.unwrap()).unwrap();
        assert_eq!(v.get("output_mode").and_then(|x| x.as_str()), Some("subtitle"));
        assert!(
            v.get("outputMode").is_none(),
            "不该出现 camelCase 变体: {v}"
        );
    }

    #[test]
    fn snake_case_field_names() {
        let input: Input =
            serde_json::from_str(r#"{"workflow":{"source_lang":"zh","target_step":"mix_video"}}"#)
                .unwrap();
        // source_lang 是开放字符串 (源语言是"事实", 不受 23 种翻译目标语言限制)
        assert_eq!(
            input.workflow.as_ref().unwrap().source_lang,
            Some(crate::r#const::lang::LangStr::from("zh"))
        );
        assert_eq!(
            input.workflow.as_ref().unwrap().target_step,
            Some(args::StepName::MixVideo)
        );
    }

    #[test]
    fn sf_ocr_flatten_fields_deserialize() {
        let input: Input = serde_json::from_str(
            r#"{"steps":{"sf_ocr":{"text_confidence_threshold":0.6},"sf_ocr_fix":{"llm_fix":true,"llm_model":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(input.steps.sf_ocr.text_confidence_threshold, 0.6);
        assert!(input.steps.sf_ocr_fix.llm_fix.llm_fix);
        assert_eq!(input.steps.sf_ocr_fix.llm_fix.llm_model, "x");
        // 默认值补齐 (absent 字段走 Rust Default, 应为预期字面默认值)
        assert_eq!(input.steps.asr_ocr_pre.fps, 2.0);
        assert_eq!(input.steps.sf_ocr.subtitle_only, true);
    }

    #[test]
    fn validate_workflow_required_for_workflow_command() {
        let ok: Input = serde_json::from_str(r#"{"command":"workflow","workflow":{}}"#).unwrap();
        assert!(ok.validate().is_ok());

        let missing: Input = serde_json::from_str(r#"{"command":"workflow"}"#).unwrap();
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

    #[test]
    fn cookie_field_wires_cookie_args() {
        let input: Input = serde_json::from_str(
            r#"{"command":"cookie","cookie":{"content":"abc","service":"youtube","action":"set"}}"#,
        )
        .unwrap();
        assert_eq!(input.command, Command::Cookie);
        let cookie = input.cookie.unwrap();
        assert_eq!(cookie.action, crate::cmd::cookie::CookieAction::Set);
        assert_eq!(cookie.service, crate::cmd::cookie::CookieService::Youtube);
        assert_eq!(cookie.content.as_deref(), Some("abc"));

        let empty: Input = serde_json::from_str(r#"{"command":"cookie"}"#).unwrap();
        assert!(empty.cookie.is_none());
    }
}
