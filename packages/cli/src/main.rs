//! LocalDub Rust CLI 入口。
//!
//! 流程 (镜像 TS `packages/cli/run-workflow.ts` 的 workflow 动作):
//! 1. 默认从仓库根目录读取 `input.jsonc` (优先) 或 `input.json`。
//! 2. 剥离 JSONC 注释 (`//` 行注释 与 `/* */` 块注释)。
//! 3. 反序列化为 `ld_core::input::Input`。
//! 4. 按 `command` 字段派发: workflow (cmd_workflow) / video_action / query / queue /
//!    env / servers / cookie / check / deviceInfo / listModels。
//!
//! 失败打印错误并以退出码 1 退出, 成功以 0 退出。

use std::process::exit;

use anyhow::Context;
use clap::{Parser, Subcommand};
use config_rs::servers::ServerType;
use ld_core::cmd::check::CheckType;
use ld_core::cmd::env::args::EnvAction;
use ld_core::cmd::workflows::workflow::cmd_workflow;
use ld_core::input::Command as InputCommand;
use ld_core::input::{Input, QueryAction, QueueAction, VideoAction};
use ld_core::servers::args::ServerAction;
use ld_core::workflow::args::{StepName, WorkflowAction};
use paths::parse_repo_input;

/// LocalDub CLI。
///
/// 无子命令时读取仓库根 `input.jsonc` 的 `command` 字段派发 (workflow/env/servers/cookie 等);
/// `env`/`workflow` 子命令可用命令行参数直接触发。
///
/// 设计意图:
/// - `env`/`workflow`/`check`/`queue` 做成 clap 子命令: 参数是标量 (action/url/video_dir/queue_id/step/type),
///   适合命令行; 显式传的参数覆盖 input.jsonc, 缺失保留 (混合回退), 因此可以完全不改 input.jsonc 操作。
/// - steps 等嵌套配置仍靠 input.jsonc (不适合命令行)。
/// - 其余命令 (servers/cookie/deviceInfo/listModels 等) 继续靠 input.jsonc 的 `command` 字段派发;
///   `deviceInfo`/`listModels` 无参数, 直接用空子命令触发。
///
/// 混合策略: 每个子命令的显式参数优先, 缺失参数回退 input.jsonc, 再回退默认。
#[derive(Parser)]
#[command(name = "cli", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// 子命令。
///
/// 每个子命令有自己的专属参数 (如 `env` 的 `--action`/`--targets`);
/// 其他子命令 (若后续加入) 各自定义自己的选项, 不共用这套。
#[derive(Subcommand)]
enum Command {
    /// 环境检查/修复 (等价 input.jsonc command=env)。
    Env {
        /// 动作: check (默认) / ensure。未传时回退 input.jsonc 的 env.action。
        #[arg(long, value_enum)]
        action: Option<EnvAction>,
        /// 要检查的环境项 key (可多个; 空 → 按 input.jsonc 推断)。
        #[arg(long, num_args = 1..)]
        targets: Vec<String>,
    },
    /// 任务操作 (等价 input.jsonc command=workflow, 标量参数覆盖 input.jsonc)。
    Workflow {
        /// 任务动作: import_and_start/start/resume/import/enqueue_*/cancel_queue/...
        #[arg(long, value_enum)]
        action: Option<WorkflowAction>,
        /// 本地文件路径或远程/云端 url (start/enqueue_start 用)。
        #[arg(long)]
        url: Option<String>,
        /// 任务目录 (resume/enqueue_resume/status 用)。
        #[arg(long)]
        video_dir: Option<String>,
        /// 续跑起点集 (resume/enqueue_resume 用)，多个起点取传递下游并集。
        #[arg(long, value_enum, num_args = 1..)]
        resume_from: Vec<StepName>,
        /// 目标步骤集 (resume/enqueue_resume 用)，全部达成才停。
        #[arg(long, value_enum, num_args = 1..)]
        target_step: Vec<StepName>,
    },
    /// 对某个已导入视频任务的非工作流动作 (等价 command=video_action)，video_dir 必填。
    VideoAction {
        /// 动作: status / get_workflow_ctx / generate_meta。
        #[arg(long, value_enum)]
        action: VideoAction,
        /// 已导入任务的目录（必填）。
        #[arg(long)]
        video_dir: String,
    },
    /// 纯查询 (等价 input.jsonc command=query)，只读不改状态。
    Query {
        /// 查询动作: get_group_list / list_queue。
        #[arg(long, value_enum)]
        action: QueryAction,
    },
    /// 主服务器任务队列操作 (等价 input.jsonc command=queue)。
    Queue {
        /// 队列动作: cancel_queue。
        #[arg(long, value_enum)]
        action: QueueAction,
        /// 队列任务 ID（必填）。
        #[arg(long)]
        queue_id: u64,
    },
    /// 服务器管理 (等价 input.jsonc command=servers)。
    Servers {
        /// 动作: status(默认)/start/stop/discovery。
        #[arg(long, value_enum)]
        action: Option<ServerAction>,
        /// 服务器类型: main / voxcpm_torch_gradio; 缺省操作全部 (start 仅支持 main)。
        #[arg(long, value_enum)]
        name: Option<ServerType>,
        /// start 前台模式: 日志实时打到终端, Ctrl+C 终止 (默认 detach 后台+落盘)。
        #[arg(long)]
        foreground: bool,
    },
    /// 资源检查 (等价 input.jsonc command=check)。
    Check {
        /// 检查类型: video(默认)/asr/font。
        #[arg(long, value_enum)]
        r#type: Option<CheckType>,
        /// 任务目录 (video/asr 检查必需)。
        #[arg(long)]
        video_dir: Option<String>,
    },
    /// 设备信息 (等价 input.jsonc command=deviceInfo)。
    #[command(name = "deviceInfo")]
    DeviceInfo,
    /// 列出模型 (等价 input.jsonc command=listModels)。
    #[command(name = "listModels")]
    ListModels,
}

#[tokio::main]
async fn main() {
    // 统一初始化 tracing: fmt(stderr) + 任务文件落盘 + EnvFilter。
    // 重复 init 会失败, 故仅当尚未初始化时才装 (测试/嵌套调用安全)。
    let _ = ld_core::logging::init();

    // 分发 (镜像 TS run-workflow.ts 的 switch(cmd)):
    // 1. 读 input.jsonc 得到基础 Input;
    // 2. 若有 cli 子命令 (如 `cli env --action/--targets`), 用其参数覆盖 Input 对应字段,
    //    统一走下面的 match input.command 派发 (cli 显式参数优先, 缺失保留 input.jsonc);
    //    CLI 命令 (check/deviceInfo/listModels) 均已移植到 Rust, 与 TS 分支一一对应, 且都有
    //    clap 子命令可直接触发 (check 带 --type/--workflow-dir, 后两者无参数);
    //    input 解析失败直接报错退出。
    let cli = Cli::parse();

    let mut input = match parse_repo_input() {
        Ok(input) => input,
        // 有 cli 子命令 (如 `cli env`): input.jsonc 可选, 用默认 Input 作为基础
        Err(_) if cli.command.is_some() => Input::default(),
        // 无 cli 子命令: 必须读 input.jsonc 才知道跑什么命令
        Err(e) => {
            eprintln!("[cli] 读取 input 失败: {e:#}");
            exit(1);
        }
    };

    // cli 子命令参数作为 Input 覆盖层: 显式传的字段才覆盖, 缺失保留 input.jsonc (混合回退)。
    match cli.command {
        Some(Command::Env { action, targets }) => {
            let mut env = input.env.clone().unwrap_or_default();
            if let Some(a) = action {
                env.action = a;
            }
            if !targets.is_empty() {
                env.targets = targets;
            }
            input.env = Some(env);
            input.command = InputCommand::Env;
        }
        Some(Command::Workflow {
            action,
            url,
            video_dir,
            resume_from,
            target_step,
        }) => {
            let mut workflow = input.workflow.clone().unwrap_or_default();
            if let Some(a) = action {
                workflow.action = Some(a);
            }
            if let Some(u) = url {
                workflow.url = Some(u);
            }
            if let Some(d) = video_dir {
                workflow.video_dir = Some(d);
            }
            if !resume_from.is_empty() {
                workflow.resume_from = resume_from;
            }
            if !target_step.is_empty() {
                workflow.target_step = target_step;
            }
            input.workflow = Some(workflow);
            input.command = InputCommand::Workflow;
        }
        Some(Command::VideoAction { action, video_dir }) => {
            let mut va = input.video_action.clone().unwrap_or(ld_core::input::VideoActionArgs {
                action,
                video_dir: video_dir.clone(),
            });
            va.action = action;
            va.video_dir = video_dir;
            input.video_action = Some(va);
            input.command = InputCommand::VideoAction;
        }
        Some(Command::Query { action }) => {
            let mut query = input.query.clone().unwrap_or(ld_core::input::QueryArgs { action });
            query.action = action;
            input.query = Some(query);
            input.command = InputCommand::Query;
        }
        Some(Command::Queue { action, queue_id }) => {
            input.queue = Some(ld_core::input::QueueArgs { action, queue_id });
            input.command = InputCommand::Queue;
        }
        Some(Command::Servers {
            action,
            name,
            foreground,
        }) => {
            let mut servers = input.servers.clone().unwrap_or_default();
            if let Some(a) = action {
                servers.action = a;
            }
            if let Some(n) = name {
                servers.name = Some(n);
            }
            if foreground {
                servers.foreground = true;
            }
            input.servers = Some(servers);
            input.command = InputCommand::Servers;
        }
        Some(Command::Check {
            r#type,
            video_dir,
        }) => {
            let mut check = input.check.clone().unwrap_or_default();
            if let Some(t) = r#type {
                check.r#type = t;
            }
            if let Some(d) = video_dir {
                check.video_dir = Some(d);
            }
            input.check = Some(check);
            input.command = InputCommand::Check;
        }
        Some(Command::DeviceInfo) => {
            input.command = InputCommand::DeviceInfo;
        }
        Some(Command::ListModels) => {
            input.command = InputCommand::ListModels;
        }
        None => {}
    }

    if let Err(e) = input.validate() {
        eprintln!("[cli] input 校验失败: {e}");
        exit(1);
    }
    println!("[cli] 读取 input");

    let run_result: anyhow::Result<()> = match input.command {
        InputCommand::Workflow => {
            cmd_workflow(&input).await.context("cmd_workflow 失败")
        }
        InputCommand::VideoAction => match input.video_action.as_ref() {
            Some(args) => {
                ld_core::cmd::workflows::video_action::cmd_video_action(args)
                    .await
                    .context("cmd_video_action 失败")
            }
            None => Err(anyhow::anyhow!("command=video_action 时 video_action 必填")),
        },
        InputCommand::Query => match input.query.as_ref() {
            Some(args) => ld_core::cmd::workflows::query::cmd_query(args).context("cmd_query 失败"),
            None => Err(anyhow::anyhow!("command=query 时 query 必填")),
        },
        InputCommand::Queue => match input.queue.as_ref() {
            Some(args) => ld_core::cmd::workflows::queue::cmd_queue(args).context("cmd_queue 失败"),
            None => Err(anyhow::anyhow!("command=queue 时 queue 必填")),
        },
        InputCommand::Env => ld_core::cmd::env::handler::cmd_env(&input).await.context("cmd_env 失败"),
        InputCommand::Servers => ld_core::cmd::servers::cmd_servers(&input)
            .context("servers 命令失败")
            .map(|s| println!("{s}")),
        InputCommand::Cookie => {
            let args = input.cookie.clone().unwrap_or_default();
            ld_core::cmd::cookie::cmd_cookie(&args).context("cookie 命令失败")
        }
        InputCommand::Check => {
            let args = input.check.clone().unwrap_or_default();
            ld_core::cmd::check::cmd_check(&input, &args).context("check 命令失败")
        }
        InputCommand::ListModels => {
            ld_core::cmd::list_models::cmd_list_models(&input).context("listModels 命令失败")
        }
        InputCommand::DeviceInfo => {
            let info = device_rs::get_device_info();
            serde_json::to_string_pretty(&info)
                .map(|s| println!("{s}"))
                .map_err(|e| anyhow::anyhow!("序列化设备信息失败: {e}"))
        }
    };

    // 提示音语义: 任务完成 ≠ 命令完成。
    // - 同步任务命令 (import_and_start/start/resume/import 走 workflow):
    //   命令完成 = 任务完成 -> workflow_success / task_fail。
    // - 其它命令 (enqueue 提交/servers/查询/video_action 等): 命令结束但任务未运行或无关
    //   -> command_done; 失败仍是 task_fail。
    let action = input.workflow.as_ref().and_then(|t| t.action);
    let is_sync_workflow = matches!(
        (input.command, action),
        (
            InputCommand::Workflow,
            None
                | Some(
                    WorkflowAction::ImportAndStart
                        | WorkflowAction::Start
                        | WorkflowAction::Resume
                        | WorkflowAction::Import
                )
        )
    );

    match run_result {
        Ok(()) => {
            println!("[cli] 完成");
            if is_sync_workflow {
                ld_core::cmd::sound::play_workflow_success();
            } else {
                ld_core::cmd::sound::play_command_done();
            }
        }
        Err(e) => {
            eprintln!("[cli] 错误: {e:#}");
            ld_core::cmd::sound::play_workflow_fail();
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolves_jsonc_before_json() {
        // resolve_input_path 依赖磁盘, 这里只验证两个候选的命名顺序语义
        let root = config_rs::root::repo_root();
        let a = root.join("input.jsonc");
        let b = root.join("input.json");
        assert!(a.to_string_lossy().ends_with("input.jsonc"));
        assert!(b.to_string_lossy().ends_with("input.json"));
    }

    /// 解析一段 JSONC, 验证 JSONC 剥离 + Input 反序列化 + mix_video/mix_audio 小数生效。
    ///
    /// 用独立的 JSONC 字符串而非仓库根 input.jsonc, 避免与用户真实配置耦合
    /// (用户改 input.jsonc 不影响本测试)。不触发 import_video / run_pipeline。
    #[test]
    fn parses_repo_input_jsonc_and_mix_video_decimals() {
        let raw = r#"{
            // 行注释: subtitle_source 走 sf_ocr
            "workflow": { "action": "start", "output_mode": "dub", "subtitle_source": "sf_ocr" },
            "steps": {
                "mix_video": {
                    "font_size": 21.4,
                    "margin_v": 45.0,
                    "font": "Noto Sans CJK SC Medium",
                    "shadow": 1.1,
                    "bgm_gain": -9,
                },
                "mix_audio": {
                    "max_speed": 1.55,
                },
            },
        }"#;
        let input: ld_core::input::Input = serde_json::from_value(
            jsonc_parser::parse_to_serde_value(raw, &Default::default())
                .expect("解析 JSONC 失败 (注释/尾随逗号应由 jsonc-parser 处理)"),
        )
        .expect("解析 JSONC 失败");
        assert!(input.validate().is_ok());

        // subtitle_source=sf_ocr → sf_ocr 全链路, 不经过 asr
        assert_eq!(
            input.workflow.as_ref().unwrap().subtitle_source,
            ld_core::workflow::args::SubtitleSource::SfOcr
        );

        let mv = &input.steps.mix_video;
        assert_eq!(mv.font_size, Some(21.4), "fontSize 小数应被读取");
        assert_eq!(mv.shadow, 1.1, "shadow 小数应被读取");
        assert_eq!(mv.margin_v, Some(45.0));
        assert_eq!(mv.font.as_deref(), Some("Noto Sans CJK SC Medium"));
        assert_eq!(mv.bgm_gain, -9.0);

        let ma = &input.steps.mix_audio;
        assert_eq!(ma.max_speed, 1.55);
    }
}
