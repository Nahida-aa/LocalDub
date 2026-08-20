//! cli 工具包的共享工具函数 (供 `cli` 与 `inputctl` 两个 bin 复用)。
//!
//! 目前有 JSONC 剥离 + 仓库根 input 定位/解析。

use anyhow::Context;

/// 定位 input 文件: 仓库根目录优先 `input.jsonc`, 其次 `input.json`。
pub fn resolve_input_path() -> anyhow::Result<std::path::PathBuf> {
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

/// 解析仓库根 `input.jsonc`/`input.json` 为 `ld_core::input::Input`。
/// 解析失败返回 Err (调用方可选择回退/报错)。
pub fn parse_repo_input() -> anyhow::Result<ld_core::input::Input> {
    let path = resolve_input_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取 input 失败: {}", path.display()))?;
    let cleaned = strip_jsonc_comments(&raw);
    let input: ld_core::input::Input = serde_json::from_str(&cleaned)
        .with_context(|| format!("解析 input JSON 失败: {}", path.display()))?;
    Ok(input)
}

/// 剥离 JSONC 注释: `//` 行注释 与 `/* ... */` 块注释。
///
/// 同时移除尾随逗号 (JSONC 允许, 但 `serde_json` 不允许), 例如
/// `{ "a": 1, }` → `{ "a": 1 }`。不依赖正则 (遵循 AGENTS.md "no regex dep"
/// 约定), 逐字符状态机处理。字符串内 (单/双引号) 的注释符号 / 逗号原样保留。
///
/// 注意: 必须按 `char` (而非 `u8`) 迭代, 否则多字节 UTF-8 (中文路径等) 会被
/// `as char` 当成 Latin-1 码点而双重编码, 导致路径/模型名损坏。
pub fn strip_jsonc_comments(src: &str) -> String {
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
}
