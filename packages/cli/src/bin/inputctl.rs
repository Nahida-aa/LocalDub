//! `inputctl` — LocalDub `input.jsonc` / `input.json` 编辑/查询/校验工具。
//!
//! 解决 JSONC (带注释 + 尾随逗号) 难以用 `jq` / `python -m json` 安全修改的问题:
//! - `get <path>`      读取字段 (如 `task.continueFrom`, `stages.tts.runtime`)
//! - `set <path> <val>` 写入字段; 默认**保留 JSONC 注释/格式** (CST span 级替换),
//!                       加 `--json` 则整体重写为标准 JSON (丢注释)
//! - `validate`        用 ld_core::Input 反序列化 + .validate() 做宽松校验
//! - `path`            打印实际解析到的 input 文件路径 (优先 input.jsonc)
//!
//! 位置参数风格, 无子命令则打印用法。失败以退出码 1 退出。

use std::process::exit;

use anyhow::{Context, anyhow};
use jsonc_parser::CollectOptions;
use jsonc_parser::ParseOptions;
use jsonc_parser::ast::Value as JsoncValue;
use jsonc_parser::common::Ranged;
use jsonc_parser::parse_to_ast;

/// 剥离 JSONC 注释 + 尾随逗号, 转成合法 JSON 文本 (供 serde_json 解析)。
///
/// 与 cli/main.rs 的 `strip_jsonc_comments` 同源, 这里独立复制一份 (工具二进制,
/// 不耦合 cli 私有符号)。按 `char` 迭代以正确处理多字节 UTF-8 (中文路径)。
fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_line = false;
    let mut in_block = false;
    let mut in_str: char = '\0';

    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str != '\0' {
            out.push(c);
            if c == '\\' {
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
        if c == '"' || c == '\'' {
            in_str = c;
            out.push(c);
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    chars.next();
                    in_line = true;
                    continue;
                }
                Some('*') => {
                    chars.next();
                    in_block = true;
                    continue;
                }
                _ => {}
            }
        }
        if c == '}' || c == ']' {
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

/// 定位 input 文件: 优先仓库根 `input.jsonc`, 其次 `input.json`。
fn resolve_input_path() -> anyhow::Result<std::path::PathBuf> {
    let root = config_rs::root::repo_root();
    let candidates = [root.join("input.jsonc"), root.join("input.json")];
    for c in candidates.iter() {
        if c.exists() {
            return Ok(c.clone());
        }
    }
    Err(anyhow!(
        "未找到 input 文件: 期望 {}/input.jsonc 或 {}/input.json",
        root.display(),
        root.display()
    ))
}

/// 把命令行传入的 `<val>` 解析成 serde_json::Value (用于写回)。
///
/// 规则: `true`/`false`/`null` → 对应类型; 能解析为 JSON 数字 → 数字;
/// 以 `{` / `[` 开头 → 当作 JSON 值; 否则 → 字符串 (若本就被引号包裹则去引号)。
fn parse_cli_value(raw: &str) -> anyhow::Result<serde_json::Value> {
    match raw {
        "true" => return Ok(serde_json::Value::Bool(true)),
        "false" => return Ok(serde_json::Value::Bool(false)),
        "null" => return Ok(serde_json::Value::Null),
        _ => {}
    }
    if raw.starts_with('{') || raw.starts_with('[') {
        return serde_json::from_str(raw).with_context(|| format!("解析 JSON 值失败: {raw}"));
    }
    // 去掉可能的外层引号
    let unquoted = if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    // 尝试当作数字
    if let Ok(n) = unquoted.parse::<serde_json::Number>() {
        return Ok(serde_json::Value::Number(n));
    }
    Ok(serde_json::Value::String(unquoted.to_string()))
}

/// 把 serde_json::Value 序列化为可 splice 进源文本的 JSON 字面量。
fn value_to_json_literal(v: &serde_json::Value) -> String {
    serde_json::to_string(v).expect("serde_json::Value 必可序列化")
}

/// 非保留模式: 用 serde_json 整体重写 (丢注释)。
fn set_non_preserving(
    text: &str,
    path: &[String],
    value: &serde_json::Value,
) -> anyhow::Result<String> {
    let cleaned = strip_jsonc(text);
    let mut doc: serde_json::Value = serde_json::from_str(&cleaned)
        .with_context(|| "解析 input JSON 失败 (非保留模式)".to_string())?;
    set_in_json(&mut doc, path, value)
        .with_context(|| format!("设置路径 {} 失败", path.join(".")))?;
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// 在 serde_json::Value 上按 path 设值 (非保留模式用)。
fn set_in_json(
    doc: &mut serde_json::Value,
    path: &[String],
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    if path.is_empty() {
        return Err(anyhow!("空路径"));
    }
    let mut cur = doc;
    for (i, seg) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;
        if is_last {
            if let serde_json::Value::Object(map) = cur {
                map.insert(seg.clone(), value.clone());
                return Ok(());
            } else if let serde_json::Value::Array(arr) = cur {
                let idx: usize = seg
                    .parse()
                    .with_context(|| format!("数组索引期望数字, 得到 {seg}"))?;
                if idx >= arr.len() {
                    return Err(anyhow!("数组索引 {idx} 越界 (长度 {})", arr.len()));
                }
                arr[idx] = value.clone();
                return Ok(());
            }
            return Err(anyhow!(
                "路径 {} 的父节点不是对象或数组",
                path[..i].join(".")
            ));
        }
        // 中间节点: 确保存在且为目标类型
        if let serde_json::Value::Object(map) = cur {
            cur = map
                .entry(seg.clone())
                .or_insert_with(|| serde_json::Value::Object(Default::default()));
        } else if let serde_json::Value::Array(arr) = cur {
            let idx: usize = seg
                .parse()
                .with_context(|| format!("数组索引期望数字, 得到 {seg}"))?;
            while arr.len() <= idx {
                arr.push(serde_json::Value::Null);
            }
            cur = &mut arr[idx];
        } else {
            return Err(anyhow!(
                "路径 {} 的父节点不是对象或数组",
                path[..i].join(".")
            ));
        }
    }
    Ok(())
}

/// 保留模式: 解析成 CST, 按 path 定位目标 value 节点, 对其 source span 做外科手术式替换。
fn set_preserving(
    text: &str,
    path: &[String],
    value: &serde_json::Value,
) -> anyhow::Result<String> {
    let parsed = parse_to_ast(
        text,
        &CollectOptions::default(),
        &ParseOptions {
            allow_comments: true,
            allow_trailing_commas: true,
            allow_loose_object_property_names: true,
            allow_single_quoted_strings: true,
            ..Default::default()
        },
    )
    .map_err(|e| anyhow!("JSONC 解析失败: {e:?}"))?;

    let root = parsed
        .value
        .ok_or_else(|| anyhow!("input 文件为空或不是合法 JSON 值"))?;

    let target = find_value_node(&root, path).ok_or_else(|| {
        anyhow!(
            "找不到路径 {} (保留模式下中间节点必须已存在)",
            path.join(".")
        )
    })?;

    let range = target.range();
    let start = range.start;
    let end = range.end;
    if end < start || end > text.len() {
        return Err(anyhow!("内部错误: 目标节点 span 越界"));
    }
    let replacement = value_to_json_literal(value);
    let mut out = String::with_capacity(text.len() - (end - start) + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(&replacement);
    out.push_str(&text[end..]);
    Ok(out)
}

/// 在 CST Value 上按 path 递归定位目标节点 (返回其引用, 借自源文本)。
fn find_value_node<'a>(node: &'a JsoncValue<'a>, path: &[String]) -> Option<&'a JsoncValue<'a>> {
    if path.is_empty() {
        return Some(node);
    }
    let (head, tail) = path.split_first().unwrap();
    match node {
        JsoncValue::Object(obj) => {
            let prop = obj.get(head)?;
            find_value_node(&prop.value, tail)
        }
        JsoncValue::Array(arr) => {
            let idx: usize = head.parse().ok()?;
            let elem = arr.elements.get(idx)?;
            find_value_node(elem, tail)
        }
        _ => None,
    }
}

/// 非保留模式读取 (serde_json)。
fn get_non_preserving(text: &str, path: &[String]) -> anyhow::Result<serde_json::Value> {
    let cleaned = strip_jsonc(text);
    let doc: serde_json::Value =
        serde_json::from_str(&cleaned).with_context(|| "解析 input JSON 失败 (get)".to_string())?;
    get_in_json(&doc, path)
}

fn get_in_json(doc: &serde_json::Value, path: &[String]) -> anyhow::Result<serde_json::Value> {
    let mut cur = doc;
    for seg in path {
        match cur {
            serde_json::Value::Object(map) => {
                cur = map.get(seg).ok_or_else(|| anyhow!("路径段 {seg} 不存在"))?;
            }
            serde_json::Value::Array(arr) => {
                let idx: usize = seg.parse().with_context(|| format!("数组索引 {seg}"))?;
                cur = arr.get(idx).ok_or_else(|| anyhow!("数组索引 {idx} 越界"))?;
            }
            _ => return Err(anyhow!("路径 {seg} 的父节点不是对象/数组")),
        }
    }
    Ok(cur.clone())
}

fn usage() -> String {
    "\
用法:
  inputctl get <path>            读取字段 (如 task.continueFrom / stages.tts.runtime)
  inputctl set <path> <value>   写入字段 (默认保留 JSONC 注释/格式)
  inputctl set --json <path> <value>  写入并整体重写为标准 JSON (丢注释)
  inputctl validate             用 ld_core::Input 做宽松校验
  inputctl path                打印实际解析到的 input 文件路径

示例:
  inputctl get task.taskDir
  inputctl set task.continueFrom mix_video
  inputctl set stages.tts.runtime cloud
  inputctl set task.onlyIndices '[1,2,3]'
  inputctl validate
"
    .to_string()
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("{}", usage());
        return Ok(());
    }

    match args[0].as_str() {
        "path" => {
            let p = resolve_input_path()?;
            println!("{}", p.display());
            Ok(())
        }
        "validate" => {
            let p = resolve_input_path()?;
            let raw = std::fs::read_to_string(&p)
                .with_context(|| format!("读取 input 失败: {}", p.display()))?;
            let cleaned = strip_jsonc(&raw);
            let input: ld_core::input::Input = serde_json::from_str(&cleaned)
                .with_context(|| format!("解析 input 失败: {}", p.display()))?;
            input
                .validate()
                .map_err(|e| anyhow!("input 校验失败: {e}"))?;
            println!(
                "[inputctl] OK: {} 解析通过 ld_core::Input 校验",
                p.display()
            );
            Ok(())
        }
        "get" => {
            let path = args.get(1).ok_or_else(|| anyhow!("缺少 <path> 参数"))?;
            let segs = split_path(path)?;
            let p = resolve_input_path()?;
            let raw = std::fs::read_to_string(&p)
                .with_context(|| format!("读取 input 失败: {}", p.display()))?;
            let v = get_non_preserving(&raw, &segs)?;
            // 字符串原样打印 (不带引号), 其他 pretty 打印
            match v {
                serde_json::Value::String(s) => println!("{s}"),
                other => println!("{}", serde_json::to_string_pretty(&other)?),
            }
            Ok(())
        }
        "set" => {
            let mut i = 1;
            let mut preserve = true;
            if args.get(i).map(|s| s.as_str()) == Some("--json") {
                preserve = false;
                i += 1;
            }
            let path = args.get(i).ok_or_else(|| anyhow!("缺少 <path> 参数"))?;
            let value_raw = args
                .get(i + 1)
                .ok_or_else(|| anyhow!("缺少 <value> 参数"))?;
            let segs = split_path(path)?;
            let value = parse_cli_value(value_raw)?;

            let p = resolve_input_path()?;
            let raw = std::fs::read_to_string(&p)
                .with_context(|| format!("读取 input 失败: {}", p.display()))?;
            let new_text = if preserve {
                set_preserving(&raw, &segs, &value)
                    .with_context(|| "保留模式写回失败 (中间节点需已存在)".to_string())?
            } else {
                set_non_preserving(&raw, &segs, &value)?
            };
            std::fs::write(&p, &new_text).with_context(|| format!("写回 {} 失败", p.display()))?;
            println!(
                "[inputctl] 已更新 {} = {} ({})",
                path,
                value_raw,
                if preserve {
                    "保留 JSONC"
                } else {
                    "标准 JSON"
                }
            );
            Ok(())
        }
        other => Err(anyhow!("未知子命令: {other}\n\n{}", usage())),
    }
}

/// 把 `a.b.c` 或 `arr.0` 拆成 path 段。数组索引用纯数字段表示。
fn split_path(path: &str) -> anyhow::Result<Vec<String>> {
    if path.trim().is_empty() {
        return Err(anyhow!("空路径"));
    }
    Ok(path.split('.').map(|s| s.to_string()).collect())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[inputctl] 错误: {e:#}");
        exit(1);
    }
}
