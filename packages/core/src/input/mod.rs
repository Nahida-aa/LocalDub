//! 顶层输入类型 (不限定 CLI 场景, Tauri RPC / pipeline 均可复用)。
//!
//! 镜像 TS 侧 `packages/core/input/types.ts`：
//! - `workflow` args → [`workflow::args`](crate::workflow::args)
//! - 各 pipeline 阶段参数 → [`steps`](steps)
//!
//! input 语义由 `specta_serde::PhasesFormat` 驱动：`#[serde(default)]` 的字段在
//! Deserialize 面（input）可选、在 Serialize 面（output）必填，对齐 zod 的 io 区分。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::cmd::check::CheckArgs;
use crate::cmd::cookie::CookieArgs;
use crate::cmd::env::args::EnvArgs;
use crate::servers::args::ServersArgs;
use crate::workflow::args;

pub mod steps;

/// OpenAI 兼容 provider：命名表的一项。步骤只写 provider 名 + model，
/// 由 [`WorkflowInput::llm`] 解析出 base_url / api_key。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct LlmProvider {
    /// API base (含协议与 host，不含 /chat/completions)
    pub base_url: String,
    /// 该 provider 的默认模型名。步骤未显式写 model 时用它。
    #[serde(default)]
    pub default_model: Option<String>,
    /// API key (Bearer)。缺失时回退环境变量 OPENAI_API_KEY。
    #[serde(default)]
    pub api_key: Option<String>,
}

impl LlmProvider {
    /// 取最终模型名：步骤显式 model 优先，其次 provider.default_model，
    /// 都没有则明确报错（不再有硬编码兜底）。
    pub fn resolve_model(&self, step_model: Option<&str>, provider_name: &str) -> anyhow::Result<String> {
        step_model
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| self.default_model.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "未指定模型：步骤未配置 model 且 provider '{provider_name}' 无 default_model"
                )
            })
    }
}

/// 统一的 provider 解析：按名字精确匹配；找不到且表里**恰好只有一个**时，
/// 把唯一的那个当默认（这样只配一个 provider 时步骤无需纠结名字）。
/// 所有调用点（步骤 / env 检查 / list_models / meta）共用此规则，避免分叉。
pub fn resolve_llm_provider<'a>(
    providers: &'a HashMap<String, LlmProvider>,
    name: &str,
) -> Option<&'a LlmProvider> {
    if let Some(p) = providers.get(name) {
        return Some(p);
    }
    if providers.len() == 1 {
        return providers.values().next();
    }
    None
}

/// 命令
///
/// 命名对齐 TS `commandList` (types.ts): `deviceInfo`/`listModels` 是 camelCase,
/// 其余为全小写 (增强 `rename_all = "lowercase"` 无法表达, 显式 rename)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Command {
    Workflow,
    /// 对某个已导入视频任务的动作（start/resume/status/get_workflow_ctx/generate_meta）。
    /// video_dir 必填——由 VideoActionArgs 的类型保证，不再是运行时检查。
    VideoAction,
    /// 纯查询命令：不针对单个任务（get_group_list / list_queue）。
    Query,
    Env,
    Servers,
    Cookie,
    Check,
    #[serde(rename = "deviceInfo")]
    DeviceInfo,
    #[serde(rename = "listModels")]
    ListModels,
}

/// `command=video_action` 的动作：都针对一个已导入的视频任务（video_dir 必填）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, clap::ValueEnum)]
#[value(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum VideoAction {
    /// 对已导入目录从头跑全量（不 import、不 setCtx、不跳过）。
    Start,
    /// 续跑（setCtx + DagPlan 从源 run 复用未失效步骤）。
    Resume,
    /// 打印 view.json 里的 workflow.status + 各 step 状态。
    Status,
    /// 打印 view.json / input.json / 各 <step>/state.json 三份状态。
    GetWorkflowCtx,
    /// 从 mix_video/zh.srt 生成 meta（需要 steps.meta + llm_providers）。
    GenerateMeta,
}

/// `command=video_action` 的参数。video_dir 类型层必填（非 Option）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct VideoActionArgs {
    pub action: VideoAction,
    /// 已导入任务的目录（必填）。
    pub video_dir: String,
    /// start 用：本地文件路径或远程 url（对已导入目录通常省略）。
    #[serde(default)]
    pub url: Option<String>,
    /// resume 用：续跑起点集。
    #[serde(default)]
    pub resume_from: Vec<crate::workflow::args::StepName>,
    /// resume 用：目标步骤集。
    #[serde(default)]
    pub target_step: Vec<crate::workflow::args::StepName>,
}

/// `command=query` 的查询动作（不针对单个任务）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, clap::ValueEnum)]
#[value(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum QueryAction {
    /// 列出所有任务分组。
    GetGroupList,
    /// 列出主服务器任务队列 (fnrpc list_queue)。
    ListQueue,
}

/// `command=query` 的参数。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct QueryArgs {
    pub action: QueryAction,
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
    /// video_action 参数, 仅 command=video_action 时必须
    #[serde(default)]
    pub video_action: Option<VideoActionArgs>,
    /// query 参数, 仅 command=query 时必须
    #[serde(default)]
    pub query: Option<QueryArgs>,
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
    /// 命名 LLM provider 表 (步骤按名字引用 base_url/api_key)
    #[serde(default)]
    pub llm_providers: HashMap<String, LlmProvider>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            workflow: None,
            video_action: None,
            query: None,
            command: Command::default(),
            servers: None,
            env: None,
            cookie: None,
            check: None,
            steps: steps::Steps::default(),
            llm_providers: HashMap::new(),
        }
    }
}

impl Input {
    /// 任务目录 = `input.workflow.video_dir`。
    ///
    /// 引擎入口（[`crate::integration::workflow::engine`]）与队列仍用的
    /// 串行 pipeline（[`crate::workflow::pipeline::run_pipeline`]）的 video_dir
    /// 都从这里取，不再由调用方显式传参：start 路径由 import_video
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
        if self.command == Command::Query && self.query.is_none() {
            return Err("command=query 时 query 必填".into());
        }
        if self.command == Command::VideoAction && self.video_action.is_none() {
            return Err("command=video_action 时 video_action 必填".into());
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
/// 只保留步骤真正读的字段（`workflow` / `steps` / `llm_providers`）；其余命令参数
/// （command/servers/env/cookie/check）属于 CLI/RPC 层，步骤从不读。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInput {
    pub workflow: args::WorkflowArgs,
    pub steps: steps::Steps,
    /// 命名 LLM provider 表（步骤按名字引用）
    pub llm_providers: HashMap<String, LlmProvider>,
}

impl WorkflowInput {
    /// 任务目录——由 [`TryFrom<Input>`] 保证 `workflow.video_dir` 为 Some。
    pub fn video_dir(&self) -> &str {
        self.workflow
            .video_dir
            .as_deref()
            .expect("WorkflowInput 构造时已校验 video_dir")
    }

    /// 按名字解析 LLM provider；查不到时明确报错（不静默回退）。
    /// 唯一 provider 时忽略名字直接用它（见 [`resolve_llm_provider`]）。
    pub fn llm(&self, name: &str) -> anyhow::Result<&LlmProvider> {
        resolve_llm_provider(&self.llm_providers, name)
            .ok_or_else(|| anyhow::anyhow!("未配置 llm provider: {name}"))
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
            llm_providers: input.llm_providers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_provider_is_default_regardless_of_name() {
        let mut providers = HashMap::new();
        providers.insert(
            "agnes".to_string(),
            LlmProvider {
                base_url: "https://apihub.agnes-ai.com/v1".to_string(),
                default_model: None,
                api_key: None,
            },
        );
        // 唯一 provider：写它的真名、写 "default"、写任意名字都解析到它
        for name in ["agnes", "default", "whatever"] {
            let p = resolve_llm_provider(&providers, name).unwrap();
            assert_eq!(p.base_url, "https://apihub.agnes-ai.com/v1");
        }
        // 多个 provider 时严格按名字，找不到就 None
        providers.insert(
            "local".to_string(),
            LlmProvider {
                base_url: "http://localhost:11434/v1".to_string(),
                default_model: None,
                api_key: None,
            },
        );
        assert!(resolve_llm_provider(&providers, "nonexistent").is_none());
        assert_eq!(
            resolve_llm_provider(&providers, "local").unwrap().base_url,
            "http://localhost:11434/v1"
        );
    }

    #[test]
    fn model_precedence_step_then_provider_then_error() {
        let p = LlmProvider {
            base_url: "http://localhost:11434/v1".to_string(),
            default_model: Some("provider-model".to_string()),
            api_key: None,
        };
        // 步骤显式 model 优先
        assert_eq!(p.resolve_model(Some("step-model"), "p").unwrap(), "step-model");
        // 步骤未写 → provider 默认
        assert_eq!(p.resolve_model(None, "p").unwrap(), "provider-model");
        // 都没有 → 明确报错
        let bare = LlmProvider {
            base_url: "http://localhost:11434/v1".to_string(),
            default_model: None,
            api_key: None,
        };
        assert!(bare.resolve_model(None, "bare").is_err());
    }

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
            serde_json::from_str(r#"{"workflow":{"source_lang":"zh","target_step":["mix_video"]}}"#)
                .unwrap();
        // source_lang 是开放字符串 (源语言是"事实", 不受 23 种翻译目标语言限制)
        assert_eq!(
            input.workflow.as_ref().unwrap().source_lang,
            Some(crate::r#const::lang::LangStr::from("zh"))
        );
        assert_eq!(
            input.workflow.as_ref().unwrap().target_step,
            vec![args::StepName::MixVideo]
        );
    }

    #[test]
    fn sf_ocr_flatten_fields_deserialize() {
        let input: Input = serde_json::from_str(
            r#"{"steps":{"sf_ocr":{"text_confidence_threshold":0.6},"sf_ocr_fix":{"llm_fix":true,"model":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(input.steps.sf_ocr.text_confidence_threshold, 0.6);
        assert!(input.steps.sf_ocr_fix.llm_fix.llm_fix);
        assert_eq!(input.steps.sf_ocr_fix.llm_fix.model.as_deref(), Some("x"));
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

        // command=query 时 query 必填（get_group_list / list_queue）
        let ok: Input = serde_json::from_str(r#"{"command":"query","query":{"action":"get_group_list"}}"#).unwrap();
        assert!(ok.validate().is_ok());
        assert_eq!(ok.command, Command::Query);
        assert_eq!(ok.query.as_ref().unwrap().action, QueryAction::GetGroupList);

        let ok: Input = serde_json::from_str(r#"{"command":"query","query":{"action":"list_queue"}}"#).unwrap();
        assert!(ok.validate().is_ok());
        assert_eq!(ok.query.as_ref().unwrap().action, QueryAction::ListQueue);

        let missing: Input = serde_json::from_str(r#"{"command":"query"}"#).unwrap();
        assert!(missing.validate().is_err());

        // command=video_action 时 video_action 必填，video_dir 类型层必填
        let ok: Input = serde_json::from_str(
            r#"{"command":"video_action","video_action":{"action":"status","video_dir":"workfolder/x"}}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());
        assert_eq!(ok.command, Command::VideoAction);
        assert_eq!(ok.video_action.as_ref().unwrap().video_dir, "workfolder/x");
        assert_eq!(ok.video_action.as_ref().unwrap().action, VideoAction::Status);

        // 缺 video_dir → 反序列化失败（类型层保证，不是 validate）
        let missing_dir: Result<Input, _> = serde_json::from_str(
            r#"{"command":"video_action","video_action":{"action":"status"}}"#,
        );
        assert!(missing_dir.is_err(), "video_dir 必填应由反序列化拦下");

        let missing: Input = serde_json::from_str(r#"{"command":"video_action"}"#).unwrap();
        assert!(missing.validate().is_err());
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
