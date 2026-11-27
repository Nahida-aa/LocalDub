//! 归一化器：把 agnes 自造的 arguments 形状（action/content/argument/param1/...）
//! 统一解析成目标格式 {"1":"译1", "2":"译2", ...}。
//! 用 mitmproxy 抓到的真实形态做验证。

use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Issue(pub String);

pub fn normalize(args: &serde_json::Value, n: usize) -> Result<BTreeMap<String, String>, Issue> {
    // 包装字段名不可预测（agnes 实测用过 argument/arguments/content/action/tool 等），
    // 不硬编码字段列表：递归收集所有候选字符串，逐个尝试解析，第一个成功的即译文。
    let mut candidates = Vec::new();
    collect_candidates(args, &mut candidates);
    // 能 parse 成 JSON 对象/数组的候选优先（译文通常是 JSON 字符串）。
    candidates.sort_by_key(|s| if is_json_container(s) { 0 } else { 1 });
    let mut first_err = None;
    for c in candidates {
        match parse_carrier(&c, n) {
            Ok(out) => return Ok(out),
            Err(e) => if first_err.is_none() { first_err = Some(e) },
        }
    }
    Err(first_err.unwrap_or_else(|| Issue("找不到译文载体".into())))
}

fn is_json_container(s: &str) -> bool {
    matches!(serde_json::from_str::<serde_json::Value>(s), Ok(v) if v.is_object() || v.is_array())
}

/// 递归收集所有可能装译文的字符串（含对象/数组本身的序列化形态）。
fn collect_candidates(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Object(obj) => {
            // 对象里若有数字/尾数字 key，整个对象可能是译文 → 序列化为候选
            if obj.keys().any(|k| k.parse::<usize>().is_ok()
                || k.chars().last().is_some_and(|c| c.is_ascii_digit())) {
                if let Ok(s) = serde_json::to_string(v) { out.push(s); }
            }
            for val in obj.values() { collect_candidates(val, out); }
        }
        serde_json::Value::Array(_) => {
            if let Ok(s) = serde_json::to_string(v) { out.push(s); }
        }
        _ => {}
    }
}

/// 第二层：把载体字符串解析成 序号→译文。
fn parse_carrier(s: &str, n: usize) -> Result<BTreeMap<String, String>, Issue> {
    let s = s.trim();
    // 1) JSON 对象 {"1":"译1",...}
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        if let Some(obj) = v.as_object() {
            let out: BTreeMap<String, String> = obj
                .iter()
                .filter_map(|(k, val)| {
                    let idx = k.parse::<usize>().ok()
                        .or_else(|| k.chars().last().and_then(|c| c.to_string().parse::<usize>().ok()))?;
                    Some((idx.to_string(), val.as_str()?.trim().to_string()))
                })
                .collect();
            if !out.is_empty() {
                return Ok(out);
            }
        }
        // 2) JSON 数组 ["译1",...] → 位置即序号，但必须长度恰好 == N，
        //    否则数组无序号、无法验证映射是否正确，拒绝回传让模型重填。
        if let Some(arr) = v.as_array() {
            if arr.len() == n {
                return Ok(arr.iter().enumerate()
                    .map(|(i, val)| ((i + 1).to_string(), val.as_str().unwrap_or("").trim().to_string()))
                    .collect());
            }
            return Err(Issue(format!(
                "数组长度 {} != 期望 {}，无法确认序号映射，拒绝",
                arr.len(), n
            )));
        }
    }

    // 3) 文本按行，分三种情况：
    //    a. 全有序号 → 按自带序号（同 key 对象），缺号由完整性检查报
    //    b. 全无序号 → 按位置补 1,2,3...，长度必须 == n
    //    c. 部分有 → 有序号的用自带，无序号的填到 1..N 的空洞里
    //    冲突（重复序号 / 序号超 N / 补不满）一律 REJ。
    let lines: Vec<&str> = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return Err(Issue(format!("无法从载体解析译文: {}", &s[..s.len().min(80)])));
    }
    let parsed: Vec<Option<(usize, &str)>> = lines.iter().map(|line| {
        match parse_numbered_line(line) {
            Some((idx, text)) => Some((idx, text)),
            None => None,
        }
    }).collect();
    let with_num = parsed.iter().filter(|x| x.is_some()).count();

    let mut out: BTreeMap<String, String> = BTreeMap::new();

    if with_num == lines.len() {
        // a. 全有序号：按自带序号
        for (idx, text) in parsed.into_iter().flatten() {
            if idx == 0 || idx > n {
                return Err(Issue(format!("序号 {} 超出 1..{}，拒绝", idx, n)));
            }
            if out.insert(idx.to_string(), text.to_string()).is_some() {
                return Err(Issue(format!("序号 {} 重复，拒绝", idx)));
            }
        }
        // 缺号在这里精确报告（同 key 对象规则）
        if let Some(missing) = check_missing(&out, n) {
            return Err(Issue(missing));
        }
    } else if with_num == 0 {
        // b. 全无序号：按位置补，长度必须 == n
        if lines.len() != n {
            return Err(Issue(format!(
                "无序号文本行数 {} != 期望 {}，拒绝", lines.len(), n
            )));
        }
        for (i, line) in lines.iter().enumerate() {
            out.insert((i + 1).to_string(), line.to_string());
        }
    } else {
        // c. 部分有序号：先放自带的，无序号的填空洞
        for item in &parsed {
            if let Some((idx, text)) = item {
                if *idx == 0 || *idx > n {
                    return Err(Issue(format!("序号 {} 超出 1..{}，拒绝", idx, n)));
                }
                if out.insert(idx.to_string(), text.to_string()).is_some() {
                    return Err(Issue(format!("序号 {} 重复，拒绝", idx)));
                }
            }
        }
        // 无序号行按出现顺序填入空缺位置（1..N 里还没有的）
        for line in &lines {
            if parse_numbered_line(line).is_some() { continue; }
            match first_gap(&out, n) {
                Some(gap) => { out.insert(gap.to_string(), line.to_string()); }
                None => return Err(Issue("无序号行多于空缺位置，拒绝".into())),
            }
        }
        if let Some(missing) = check_missing(&out, n) {
            return Err(Issue(missing));
        }
    }
    Ok(out)
}

/// 检查 1..n 是否齐全，返回缺号信息。
fn check_missing(out: &BTreeMap<String, String>, n: usize) -> Option<String> {
    let missing: Vec<usize> = (1..=n)
        .filter(|i| !out.contains_key(&i.to_string()))
        .collect();
    if missing.is_empty() {
        None
    } else {
        let list: Vec<String> = missing.iter().map(|i| i.to_string()).collect();
        Some(format!("缺少序号 {}，拒绝", list.join(",")))
    }
}

/// 找 1..n 里第一个没有被占用的序号（补缺用）。
fn first_gap(out: &BTreeMap<String, String>, n: usize) -> Option<usize> {
    (1..=n).find(|i| !out.contains_key(&i.to_string()))
}

fn parse_numbered_line(line: &str) -> Option<(usize, &str)> {
    // 匹配前导数字 + 分隔符（. : 、 ) 空格）+ 内容
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i == 0 || i == bytes.len() { return None; }
    let idx: usize = line[..i].parse().ok()?;
    let rest = &line[i..];
    let rest = rest.trim_start_matches(|c: char| matches!(c, '.' | ':' | '、' | ')' | '：' | ' ' | '\t'));
    if rest.is_empty() { return None; }
    Some((idx, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(args: serde_json::Value, n: usize) -> Result<BTreeMap<String, String>, String> {
        normalize(&args, n).map_err(|e| e.0)
    }

    #[test]
    fn object_keys_and_param_keys() {
        assert!(check(serde_json::json!({"1":"译1","2":"译2"}), 2).is_ok());
        assert!(check(serde_json::json!({"param1":"译1","param2":"译2"}), 2).is_ok());
    }

    #[test]
    fn wrapped_shapes_unpredictable_field_names() {
        let wrapped = serde_json::json!({"arguments":"{\"1\":\"译1\",\"2\":\"译2\"}"});
        let r = check(wrapped, 2).unwrap();
        assert_eq!(r.get("1").map(String::as_str), Some("译1"));
        // 未知字段名 + 深层嵌套也能找到
        let nested = serde_json::json!({"meta":{"output":{"whatever":"{\"1\":\"译1\",\"2\":\"译2\"}"}}});
        assert!(check(nested, 2).is_ok());
    }

    #[test]
    fn text_lines_three_cases() {
        // 全有序号：按自带，缺号/重复/超N 拒绝
        assert!(check(serde_json::json!({"content":"1. 译1\n2. 译2"}), 2).is_ok());
        assert!(check(serde_json::json!({"content":"1. 译1\n3. 译3"}), 3).is_err()); // 缺 2
        assert!(check(serde_json::json!({"content":"1. 译1\n1. 译x"}), 2).is_err()); // 重复
        // 全无序号：长度必须 == n
        assert!(check(serde_json::json!({"content":"译1\n译2\n译3"}), 3).is_ok());
        assert!(check(serde_json::json!({"content":"译1\n译2"}), 3).is_err());
        // 部分有：自带用自带，无序号填空洞，3 不记成 2
        let r = check(serde_json::json!({"content":"3. 译3\n译1\n译2"}), 3).unwrap();
        assert_eq!(r.get("3").map(String::as_str), Some("译3"));
        assert_eq!(r.get("1").map(String::as_str), Some("译1"));
        assert_eq!(r.get("2").map(String::as_str), Some("译2"));
    }

    #[test]
    fn array_requires_exact_length() {
        assert!(check(serde_json::json!(["译1","译2","译3"]), 3).is_ok());
        assert!(check(serde_json::json!(["译1","译2"]), 3).is_err());
    }
}

