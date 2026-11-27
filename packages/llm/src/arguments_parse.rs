//! tool call 的 arguments 字符串 → JSON Value 的容错解析。
//! 参考 rig 的 `json_utils`（parse_partial_object / repair_json）：
//! 协议保证 arguments 是 JSON 格式的 string，但模型可能生成非法转义或截断的 JSON。
//! 本模块在「进入 ToolInputNormalizer 之前」把能救的救回来，保证钩子拿到合法 JSON。

/// 把 arguments 字符串解析成 JSON Value，按可信度依次尝试：
/// 1. 严格 `from_str`
/// 2. `repair_json`（转义裸控制字符/非法反斜杠）后重试
/// 3. `parse_partial_object`（截断时补 `}`/`]`/引号）救援
///
/// 返回 `Ok(Value)` 时一定是合法 JSON（object/array/标量），可作为 normalizer 的输入。
pub fn parse_arguments_json(text: &str) -> anyhow::Result<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return Ok(v);
    }
    let repaired = repair_json(text);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&repaired) {
        return Ok(v);
    }
    if let Some(obj) = parse_partial_object(&repaired).or_else(|| parse_partial_object(text)) {
        return Ok(serde_json::Value::Object(obj));
    }
    Err(anyhow::anyhow!("arguments 不是合法 JSON 且无法修复: {}", truncate(text, 120)))
}

fn truncate(s: &str, max: usize) -> &str {
    let end = (0..=s.len().min(max)).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    &s[..end]
}

/// 转义字符串内的裸控制字符与不构成合法转义的反斜杠（模型常写错）。
fn repair_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !in_string {
            in_string = c == '"';
            out.push(c);
            continue;
        }
        match c {
            '"' => { in_string = false; out.push(c); }
            '\\' => match chars.peek() {
                Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                    out.push(c);
                    if let Some(next) = chars.next() { out.push(next); }
                }
                None => out.push(c), // 末尾反斜杠：可能由下一段补全
                Some(_) => out.push_str("\\\\"), // 非法转义：把反斜杠本身转义
            },
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// 截断 JSON 里"最长的、仍能构成一个 object"的前缀：补引号、补 `}`/`]`。
/// 返回 None 表示文本前缀不构成 object。
fn parse_partial_object(text: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    const MAX_ATTEMPTS: usize = 256;
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    // 每个"值刚结束"的位置，及其时的容器栈、是否需补引号。
    let mut cuts: Vec<(usize, Vec<u8>, bool)> = Vec::new();
    let bytes = text.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if in_string {
            match (escaped, *byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => { in_string = false; cuts.push((at + 1, stack.clone(), false)); }
                (false, _) => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => { stack.push(*byte); cuts.push((at + 1, stack.clone(), false)); }
            b'}' | b']' => { stack.pop(); cuts.push((at + 1, stack.clone(), false)); }
            b if b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'-' | b'+') => {
                let next = bytes.get(at + 1).copied();
                if !next.is_some_and(|n| n.is_ascii_alphanumeric() || matches!(n, b'.' | b'-' | b'+')) {
                    cuts.push((at + 1, stack.clone(), false));
                }
            }
            _ => {}
        }
    }
    if in_string {
        let at = text.len() - usize::from(escaped);
        cuts.push((at, stack.clone(), true));
    }
    cuts.iter().rev().take(MAX_ATTEMPTS).find_map(|(at, open, quote)| {
        let mut candidate = text.get(..*at)?.to_owned();
        if *quote { candidate.push('"'); }
        for c in open.iter().rev() {
            candidate.push(if *c == b'{' { '}' } else { ']' });
        }
        match serde_json::from_str(&candidate) {
            Ok(serde_json::Value::Object(obj)) => Some(obj),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_json() {
        assert!(parse_arguments_json(r#"{"1":"译1","2":"译2"}"#).is_ok());
        assert!(parse_arguments_json(r#"["a","b"]"#).is_ok());
    }

    #[test]
    fn repairs_missing_closing_brace() {
        let v = parse_arguments_json(r#"{"1":"译1","2":"译2""#).unwrap();
        assert_eq!(v["1"], "译1");
        assert_eq!(v["2"], "译2");
    }

    #[test]
    fn repairs_truncated_value() {
        // 截断在值中间：补引号 + 补 }
        let v = parse_arguments_json(r#"{"1":"译1","2":"译"#).unwrap();
        assert_eq!(v["1"], "译1");
        assert_eq!(v["2"], "译");
    }

    #[test]
    fn repairs_raw_control_chars() {
        // 字符串内有裸换行（非法 JSON），应先转义再解析
        let v = parse_arguments_json("{\"1\":\"line1\nline2\"}").unwrap();
        assert_eq!(v["1"], "line1\nline2");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_arguments_json("not json at all").is_err());
    }
}
