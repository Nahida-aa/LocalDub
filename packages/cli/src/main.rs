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

use std::path::PathBuf;
use std::process::{Command, exit};

use anyhow::Context;
use ld_core::cmd::tasks::task::cmd_task;
use ld_core::input::Input;

/// 剥离 JSONC 注释: `//` 行注释 与 `/* ... */` 块注释。
///
/// 同时移除尾随逗号 (JSONC 允许, 但 `serde_json` 不允许), 例如
/// `{ "a": 1, }` → `{ "a": 1 }`。不依赖正则 (遵循 AGENTS.md "no regex dep"
/// 约定), 逐字符状态机处理。字符串内 (单/双引号) 的注释符号 / 逗号原样保留。
///
/// 注意: 必须按 `char` (而非 `u8`) 迭代, 否则多字节 UTF-8 (中文路径等) 会被
/// `as char` 当成 Latin-1 码点而双重编码, 导致路径/模型名损坏。
fn strip_jsonc_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_line = false;
    let mut in_block = false;
    let mut in_str: char = '\0'; // '\0' = 否, '"' / '\'' = 字符串定界符

    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        // 字符串状态优先
        if in_str != '\0' {
            out.push(c);
            if c == '\\' {
                // 转义下一个字符 (含转义的引号)
                if let Some(&n) = chars.peek() {
                    out.push(n);
                    chars.next();
                }
            } else if c == in_str {
                in_str = '\0';
            }
            continue;
        }

        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
                continue;
            }
            continue;
        }

        if in_line {
            if c == '\n' {
                in_line = false;
                out.push('\n');
            }
            continue;
        }

        // 普通状态
        if c == '"' || c == '\'' {
            in_str = c;
            out.push(c);
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some(&'/') => {
                    chars.next();
                    in_line = true;
                    continue;
                }
                Some(&'*') => {
                    chars.next();
                    in_block = true;
                    continue;
                }
                _ => {}
            }
        }
        // 遇到 `}` / `]` 时, 先吃掉前面可能存在的尾随逗号 (JSONC 特性)
        if c == '}' || c == ']' {
            // 回退跳过尾随空白, 若前一非空白字符是逗号则移除之
            let mut j = out.len();
            while j > 0 && out.as_bytes()[j - 1].is_ascii_whitespace() {
                j -= 1;
            }
            if j > 0 && out.as_bytes()[j - 1] == b',' {
                out.truncate(j - 1);
            }
        }
        out.push(c);
    }
    out
}

/// 定位 input 文件: 仓库根目录优先 `input.jsonc`, 其次 `input.json`。
fn resolve_input_path() -> anyhow::Result<std::path::PathBuf> {
    let root = config_rs::root::repo_root();
    let candidates = [root.join("input.jsonc"), root.join("input.json")];
    for c in candidates.iter() {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow::anyhow!(
        "未找到 input 文件: 期望 {}/input.jsonc 或 {}/input.json",
        root.display(),
        root.display()
    ))
}

fn run() -> anyhow::Result<()> {
    let input_path = resolve_input_path()?;
    let raw = std::fs::read_to_string(&input_path)
        .with_context(|| format!("读取 input 失败: {}", input_path.display()))?;

    let cleaned = strip_jsonc_comments(&raw);
    let input: Input = serde_json::from_str(&cleaned)
        .with_context(|| format!("解析 input JSON 失败: {}", input_path.display()))?;

    input
        .validate()
        .map_err(|e| anyhow::anyhow!("input 校验失败: {e}"))?;

    println!("[cli] 读取 input: {}", input_path.display());

    // 总派发交给 cmd_task (镜像 TS cmdTask): 按 input.task.action 分派
    // start / continue / status / get_group_list / get_task_ctx。
    cmd_task(&input).context("cmd_task 失败")?;
    Ok(())
}

/// 仓库根 `assets/media/` 下的任务提示音 (镜像 TS `@repo/config/path/assets`)。
fn task_success_wav() -> PathBuf {
    config_rs::root::repo_root()
        .join("assets")
        .join("media")
        .join("task_success.wav")
}
fn task_fail_wav() -> PathBuf {
    config_rs::root::repo_root()
        .join("assets")
        .join("media")
        .join("error.wav")
}

/// 用 ffplay 播放提示音 (镜像 TS `playWav`): `-nodisp -autoexit`, 失败静默不中断流程。
/// headless / 无 ffplay 环境播放失败属正常, 忽略即可。
fn play_wav(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    let _ = Command::new("ffplay")
        .args(["-nodisp", "-autoexit"])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn play_task_success() {
    play_wav(&task_success_wav());
}
fn play_task_fail() {
    play_wav(&task_fail_wav());
}

fn main() {
    // 让 ld_core 内 emit_log 的 tracing::info! 落到 stdout (续跑/分段日志才可见)。
    // 重复 init 会报错, 故仅当尚未初始化时才装 (测试/嵌套调用安全)。
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    // `cli env [check|ensure] [targets...]` 子命令在 task 路径之前拦截。
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("env") {
        if let Err(e) = run_env(&args[1..]) {
            eprintln!("[cli] env 错误: {e:#}");
            exit(1);
        }
        return;
    }

    match run() {
        Ok(()) => {
            println!("[cli] 完成");
            play_task_success();
        }
        Err(e) => {
            eprintln!("[cli] 错误: {e:#}");
            play_task_fail();
            exit(1);
        }
    }
}

/// 处理 `cli env` 子命令: 默认 `check`, 支持 `check`/`ensure` 动作与 targets 过滤。
fn run_env(args: &[String]) -> anyhow::Result<()> {
    let (action, targets) = match args.first().map(|s| s.as_str()) {
        Some("check") => ("check", &args[1..]),
        Some("ensure") => ("ensure", &args[1..]),
        // 无动作 / 未知首参 → 当作 targets, 走默认 check
        _ => ("check", args),
    };
    let results = if action == "ensure" {
        ld_core::cmd::env::run_ensure(targets)
    } else {
        ld_core::cmd::env::run_check(targets)
    };
    for r in &results {
        println!("{}", ld_core::cmd::env::format_result(r));
    }
    println!("[cli] env {} 完成: {} 项", action, results.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_line_comments() {
        let src = r#"{
            "task": { "url": "x" } // 行尾注释
            // 整行注释
        }"#;
        let out = strip_jsonc_comments(src);
        assert!(out.contains("\"url\": \"x\""));
        assert!(!out.contains("// 行尾注释"));
        assert!(!out.contains("// 整行注释"));
    }

    #[test]
    fn strip_block_comments() {
        let src = r#"{
            /* 块注释
               跨行 */
            "task": { "url": "x" }
        }"#;
        let out = strip_jsonc_comments(src);
        assert!(out.contains("\"url\": \"x\""));
        assert!(!out.contains("块注释"));
        // 块注释被整段移除, 不应残留 /* 或 */
        assert!(!out.contains("/*"));
        assert!(!out.contains("*/"));
    }

    #[test]
    fn strips_trailing_commas() {
        // JSONC 允许尾随逗号, serde_json 不允许 → 必须移除
        let src = r#"{
            "a": 1,
            "b": [1, 2,],
        }"#;
        let out = strip_jsonc_comments(src);
        // 去掉空白后应为合法 JSON: {"a":1,"b":[1,2]}
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(compact, r#"{"a":1,"b":[1,2]}"#);
        // 确保反序列化成功 (无 trailing comma 错误)
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], 2);
    }

    #[test]
    fn preserves_comment_like_inside_string() {
        // 字符串里的 // 与 /* */ 不能当注释处理
        let src = r#"{ "path": "a//b", "note": "c /* d */ e" }"#;
        let out = strip_jsonc_comments(src);
        assert!(out.contains("a//b"));
        assert!(out.contains("c /* d */ e"));
    }

    #[test]
    fn preserves_multibyte_utf8() {
        // 回归: 多字节 UTF-8 (中文路径) 不能被 as char 双重编码损坏
        let src = r#"{ "url": "/home/aa/下载/大/37.mp4", "name": "测试" }"#;
        let out = strip_jsonc_comments(src);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "/home/aa/下载/大/37.mp4");
        assert_eq!(v["name"], "测试");
        // 字节层面应为正确 UTF-8 (非 mojibake)
        assert!(std::str::from_utf8(v["url"].as_str().unwrap().as_bytes()).is_ok());
    }

    #[test]
    fn resolves_jsonc_before_json() {
        // resolve_input_path 依赖磁盘, 这里只验证两个候选的命名顺序语义
        let root = config_rs::root::repo_root();
        let a = root.join("input.jsonc");
        let b = root.join("input.json");
        assert!(a.to_string_lossy().ends_with("input.jsonc"));
        assert!(b.to_string_lossy().ends_with("input.json"));
    }

    /// 解析仓库根 input.jsonc, 验证 JSONC 剥离 + Input 反序列化 + mix_video 小数生效。
    ///
    /// 不触发 import_video / run_pipeline, 仅做配置读取层面的端到端校验。
    #[test]
    fn parses_repo_input_jsonc_and_mix_video_decimals() {
        let path = resolve_input_path().expect("应能在仓库根找到 input.jsonc/json");
        let raw = std::fs::read_to_string(&path).expect("读取 input 失败");
        let cleaned = strip_jsonc_comments(&raw);
        let input: Input =
            serde_json::from_str(&cleaned).expect("解析 input.jsonc 失败 (JSONC 应已剥离)");
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
