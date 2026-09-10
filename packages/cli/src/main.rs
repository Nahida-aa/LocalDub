//! LocalDub Rust CLI 入口。
//!
//! 流程 (镜像 TS `packages/cli/run-task.ts` 的 task 动作):
//! 1. 默认从仓库根目录读取 `input.jsonc` (优先) 或 `input.json`。
//! 2. 剥离 JSONC 注释 (`//` 行注释 与 `/* */` 块注释)。
//! 3. 反序列化为 `ld_core::input::Input`。
//! 4. 调 `ld_core::cmd::tasks::task::cmd_task` 总派发 (镜像 TS `cmdTask`):
//!    start / continue / status / get_group_list / get_task_ctx。
//!
//! 失败打印错误并以退出码 1 退出, 成功以 0 退出。

use std::process::exit;

use anyhow::Context;
use clap::{Parser, Subcommand};
use cli::parse_repo_input;
use config_rs::servers::ServerType;
use ld_core::cmd::check::CheckType;
use ld_core::cmd::env::args::EnvAction;
use ld_core::cmd::tasks::task::cmd_task;
use ld_core::input::Command as InputCommand;
use ld_core::input::Input;
use ld_core::servers::args::ServerAction;
use ld_core::tasks::args::{StageName, TaskAction};

/// LocalDub CLI。
///
/// 无子命令时读取仓库根 `input.jsonc` 的 `command` 字段派发 (task/env/servers/cookie 等);
/// `env`/`task` 子命令可用命令行参数直接触发。
///
/// 设计意图:
/// - `env`/`task`/`check` 做成 clap 子命令: 参数是标量 (action/url/taskDir/queueId/stage/type),
///   适合命令行; 显式传的参数覆盖 input.jsonc, 缺失保留 (混合回退), 因此可以完全不改 input.jsonc 操作。
/// - stages 等嵌套配置仍靠 input.jsonc (不适合命令行)。
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
    /// 任务操作 (等价 input.jsonc command=task, 标量参数覆盖 input.jsonc)。
    Task {
        /// 任务动作: start/continue/enqueue_start/enqueue_continue/list_queue/cancel_queue/...
        #[arg(long, value_enum)]
        action: Option<TaskAction>,
        /// 本地文件路径或远程/云端 url (start/enqueue_start 用)。
        #[arg(long)]
        url: Option<String>,
        /// 任务目录 (continue/enqueue_continue/status 用)。
        #[arg(long)]
        task_dir: Option<String>,
        /// 队列任务 ID (cancel_queue 用)。
        #[arg(long)]
        queue_id: Option<u64>,
        /// 从某 stage 续跑 (continue/enqueue_continue 用)。
        #[arg(long, value_enum)]
        continue_from: Option<StageName>,
        /// 跑到此 stage 后停止 (continue/enqueue_continue 用)。
        #[arg(long, value_enum)]
        target_stage: Option<StageName>,
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
        task_dir: Option<String>,
    },
    /// 设备信息 (等价 input.jsonc command=deviceInfo)。
    #[command(name = "deviceInfo")]
    DeviceInfo,
    /// 列出模型 (等价 input.jsonc command=listModels)。
    #[command(name = "listModels")]
    ListModels,
}

fn main() {
    // 统一初始化 tracing: fmt(stderr) + 任务文件落盘 + EnvFilter。
    // 重复 init 会失败, 故仅当尚未初始化时才装 (测试/嵌套调用安全)。
    let _ = ld_core::logging::init();

    // 分发 (镜像 TS run-task.ts 的 switch(cmd)):
    // 1. 读 input.jsonc 得到基础 Input;
    // 2. 若有 cli 子命令 (如 `cli env --action/--targets`), 用其参数覆盖 Input 对应字段,
    //    统一走下面的 match input.command 派发 (cli 显式参数优先, 缺失保留 input.jsonc);
    //    CLI 命令 (check/deviceInfo/listModels) 均已移植到 Rust, 与 TS 分支一一对应, 且都有
    //    clap 子命令可直接触发 (check 带 --type/--task-dir, 后两者无参数);
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
        Some(Command::Task {
            action,
            url,
            task_dir,
            queue_id,
            continue_from,
            target_stage,
        }) => {
            let mut task = input.task.clone().unwrap_or_default();
            if let Some(a) = action {
                task.action = Some(a);
            }
            if let Some(u) = url {
                task.url = Some(u);
            }
            if let Some(d) = task_dir {
                task.task_dir = Some(d);
            }
            if let Some(q) = queue_id {
                task.queue_id = Some(q);
            }
            if let Some(cf) = continue_from {
                task.continue_from = Some(cf);
            }
            if let Some(ts) = target_stage {
                task.target_stage = Some(ts);
            }
            input.task = Some(task);
            input.command = InputCommand::Task;
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
        Some(Command::Check { r#type, task_dir }) => {
            let mut check = input.check.clone().unwrap_or_default();
            if let Some(t) = r#type {
                check.r#type = t;
            }
            if let Some(d) = task_dir {
                check.task_dir = Some(d);
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
        InputCommand::Task => cmd_task(&input).context("cmd_task 失败"),
        InputCommand::Env => ld_core::cmd::env::handler::cmd_env(&input).context("cmd_env 失败"),
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
    // - 同步任务命令 (start/continue/import, 含缺省 action 走 start):
    //   命令完成 = 任务完成 -> task_success / task_fail。
    // - 其它命令 (enqueue 提交/servers/查询等): 命令结束但任务未运行或无关
    //   -> command_done; 失败仍是 task_fail。
    let action = input.task.as_ref().and_then(|t| t.action);
    let is_sync_task = matches!(
        (input.command, action),
        (
            InputCommand::Task,
            None | Some(TaskAction::Start | TaskAction::Continue | TaskAction::Import)
        )
    );

    match run_result {
        Ok(()) => {
            println!("[cli] 完成");
            if is_sync_task {
                ld_core::cmd::sound::play_task_success();
            } else {
                ld_core::cmd::sound::play_command_done();
            }
        }
        Err(e) => {
            eprintln!("[cli] 错误: {e:#}");
            ld_core::cmd::sound::play_task_fail();
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
            "task": { "action": "start", "pipeline": "dub", "subtitleSource": "sf_ocr" },
            "stages": {
                "mix_video": {
                    "fontSize": 21.4,
                    "marginV": 45.0,
                    "font": "Noto Sans CJK SC Medium",
                    "shadow": 1.1,
                    "bgmGain": -9,
                },
                "mix_audio": {
                    "maxSpeed": 1.55,
                },
            },
        }"#;
        let input: ld_core::input::Input = serde_json::from_value(
            jsonc_parser::parse_to_serde_value(raw, &Default::default())
                .expect("解析 JSONC 失败 (注释/尾随逗号应由 jsonc-parser 处理)"),
        )
        .expect("解析 JSONC 失败");
        assert!(input.validate().is_ok());

        // subtitleSource=sf_ocr → sf_ocr 全链路, 不经过 asr
        assert_eq!(
            input.task.as_ref().unwrap().subtitle_source,
            ld_core::tasks::args::SubtitleSource::SfOcr
        );

        let mv = &input.stages.mix_video;
        assert_eq!(mv.font_size, Some(21.4), "fontSize 小数应被读取");
        assert_eq!(mv.shadow, 1.1, "shadow 小数应被读取");
        assert_eq!(mv.margin_v, Some(45.0));
        assert_eq!(mv.font.as_deref(), Some("Noto Sans CJK SC Medium"));
        assert_eq!(mv.bgm_gain, -9.0);

        let ma = &input.stages.mix_audio;
        assert_eq!(ma.max_speed, 1.55);
    }
}
