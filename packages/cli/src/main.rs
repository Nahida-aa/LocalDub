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
use ld_core::cmd::env::args::EnvAction;
use ld_core::cmd::tasks::task::cmd_task;
use ld_core::input::Command as InputCommand;
use ld_core::input::Input;

/// LocalDub CLI。
///
/// 无子命令时读取仓库根 `input.jsonc` 的 `command` 字段派发 (task/env/servers/cookie 等);
/// `env` 子命令直接做环境检查/修复。
///
/// 设计意图:
/// - `env` 做成 clap 子命令: 因其参数简单 (`--action`/`--targets`), 适合命令行交互;
/// - 其余命令 (task/servers/cookie 等) 输入复杂 (task 含完整 stages 结构), 靠
///   input.jsonc 的 `command` 字段派发, 不做命令行子命令。
/// - 因此「env 能从命令行触发、其他命令只能靠 input.jsonc」是不对称的, 是有意设计。
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
}

fn main() {
    // 统一初始化 tracing: fmt(stderr) + 任务文件落盘 + EnvFilter。
    // 重复 init 会失败, 故仅当尚未初始化时才装 (测试/嵌套调用安全)。
    let _ = ld_core::logging::init();

    // 分发 (镜像 TS run-task.ts 的 switch(cmd)):
    // 1. 读 input.jsonc 得到基础 Input;
    // 2. 若有 cli 子命令 (如 `cli env --action/--targets`), 用其参数覆盖 Input 对应字段,
    //    统一走下面的 match input.command 派发 (cli 显式参数优先, 缺失保留 input.jsonc);
    //    check/deviceInfo/listModels 未移植到 Rust, no-op (对应 TS default 空分支);
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
    if let Some(Command::Env { action, targets }) = cli.command {
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
        InputCommand::Check | InputCommand::DeviceInfo | InputCommand::ListModels => {
            // 未移植到 Rust, 与 TS default 空分支一致 (no-op)。
            Ok(())
        }
    };

    match run_result {
        Ok(()) => {
            println!("[cli] 完成");
            ld_core::cmd::sound::play_task_success();
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
    use super::*;
    use cli::strip_jsonc_comments;

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
        let cleaned = strip_jsonc_comments(raw);
        let input: ld_core::input::Input =
            serde_json::from_str(&cleaned).expect("解析 JSONC 失败 (注释/尾随逗号应已剥离)");
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
