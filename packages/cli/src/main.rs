//! LocalDub Rust CLI 入口。
//!
//! 流程 (镜像 TS `packages/cli/run-task.ts` 的 task 动作):
//! 1. 默认从仓库根目录读取 `input.jsonc` (优先) 或 `input.json`。
//! 2. 剥离 JSONC 注释 (`//` 行注释 与 `/* */` 块注释)。
//! 3. 反序列化为 `ld_core::input::Input`。
//! 4. 调用 `ld_core::tasks::import::download::import_video` 导入视频 → 写 ctx.json。
//! 5. 调用 `ld_core::stages::pipeline::run_pipeline` 跑完整 pipeline。
//!
//! 失败打印错误并以退出码 1 退出, 成功以 0 退出。

use std::process::exit;

use anyhow::Context;
use ld_core::input::Input;
use ld_core::stages::pipeline::run_pipeline;
use ld_core::tasks::import::download::import_video;

/// 剥离 JSONC 注释: `//` 行注释 与 `/* ... */` 块注释。
///
/// 同时移除尾随逗号 (JSONC 允许, 但 `serde_json` 不允许), 例如
/// `{ "a": 1, }` → `{ "a": 1 }`。不依赖正则 (遵循 AGENTS.md "no regex dep"
/// 约定), 逐字符状态机处理。字符串内 (单/双引号) 的注释符号 / 逗号原样保留。
fn strip_jsonc_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = false;
    let mut in_str: u8 = 0; // 0 = 否, b'"' / b'\'' = 字符串定界符

    while i < bytes.len() {
        let c = bytes[i];

        // 字符串状态优先
        if in_str != 0 {
            out.push(c as char);
            if c == b'\\' && i + 1 < bytes.len() {
                // 转义下一个字符 (含转义的引号)
                i += 1;
                out.push(bytes[i] as char);
            } else if c == in_str {
                in_str = 0;
            }
            i += 1;
            continue;
        }

        if in_block {
            if c == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_line {
            if c == b'\n' {
                in_line = false;
                out.push('\n');
            }
            i += 1;
            continue;
        }

        // 普通状态
        if c == b'"' || c == b'\'' {
            in_str = c;
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b'/' {
            if i + 1 < bytes.len() {
                let n = bytes[i + 1];
                if n == b'/' {
                    in_line = true;
                    i += 2;
                    continue;
                }
                if n == b'*' {
                    in_block = true;
                    i += 2;
                    continue;
                }
            }
        }
        // 遇到 `}` / `]` 时, 先吃掉前面可能存在的尾随逗号 (JSONC 特性)
        if c == b'}' || c == b']' {
            // 回退跳过尾随空白, 若前一非空白字符是逗号则移除之
            let mut j = out.len();
            while j > 0 && out.as_bytes()[j - 1].is_ascii_whitespace() {
                j -= 1;
            }
            if j > 0 && out.as_bytes()[j - 1] == b',' {
                out.truncate(j - 1);
            }
        }
        out.push(c as char);
        i += 1;
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
    let ctx = import_video(&input).context("import_video 失败")?;
    println!("[cli] 导入完成, task_dir = {}", ctx.task.task_dir);

    run_pipeline(&ctx.task.task_dir).context("run_pipeline 失败")?;
    println!("[cli] 完成: {}", ctx.task.task_dir);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[cli] 错误: {e:#}");
        exit(1);
    }
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
