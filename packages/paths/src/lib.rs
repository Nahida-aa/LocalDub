//! 仓库根 input 文件 (input.jsonc / input.json) 定位与 JSONC 解析的共享工具。
//!
//! 供 `cli`(main + inputctl) 与 `server` 复用, 避免各自重复实现
//! 「定位仓库根 input + jsonc-parser 一步解码」逻辑。

use anyhow::Context;
use jsonc_parser::parse_to_serde_value;

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

/// JSONC → serde_json::Value 一步解码 (注释/尾随逗号由 jsonc-parser 处理)。
pub fn parse_jsonc(text: &str) -> anyhow::Result<serde_json::Value> {
    parse_to_serde_value(text, &Default::default())
        .map_err(|e| anyhow::anyhow!("解析 JSONC 失败: {e:?}"))
}

/// 解析仓库根 `input.jsonc`/`input.json` 为 `ld_core::input::Input`。
/// 注释/尾随逗号由 jsonc-parser 直接处理, 无需预剥离。
/// 解析失败返回 Err (调用方可选择回退/报错)。
pub fn parse_repo_input() -> anyhow::Result<ld_core::input::Input> {
    let path = resolve_input_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取 input 失败: {}", path.display()))?;
    let input: ld_core::input::Input = parse_to_serde_value(&raw, &Default::default())
        .map_err(|e| anyhow::anyhow!("解析 input JSONC 失败 ({}): {e}", path.display()))?;
    Ok(input)
}

#[cfg(test)]
mod tests {
    use jsonc_parser::parse_to_serde_value;

    #[test]
    fn parse_jsonc_with_comments_and_trailing_commas() {
        let src = r#"{
            "a": 1, // 行注释
            /* 块
               注释 */
            "b": [1, 2,],
        }"#;
        let v: serde_json::Value = parse_to_serde_value(src, &Default::default()).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], 2);
    }

    #[test]
    fn parse_jsonc_preserves_multibyte_and_comment_like_strings() {
        let src = r#"{ "url": "/home/aa/下载/大/37.mp4", "note": "a//b c /* d */ e", }"#;
        let v: serde_json::Value = parse_to_serde_value(src, &Default::default()).unwrap();
        assert_eq!(v["url"], "/home/aa/下载/大/37.mp4");
        assert_eq!(v["note"], "a//b c /* d */ e");
        assert!(std::str::from_utf8(v["url"].as_str().unwrap().as_bytes()).is_ok());
    }
}