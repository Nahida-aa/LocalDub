//! sf_ocr_fix 的 LLM 修正 glue (镜像 TS `packages/core/ml/llm/ocr_llm_fix.ts` +
//! `srt_shared.ts::parseLines`)。
//!
//! 仅含纯函数: 构造 prompt / 解析 LLM 回复 / 调 [`crate::chat_completions`]。不含任何二进制
//! 调用或 IO, 方便单元测试。

use crate::LlmFixArgs;
use anyhow::anyhow;

/// 解析 LLM 按行号返回的修正文本。
///
/// 镜像 TS `parseLines`: 匹配 `^\s*\d+\s*[):.]\s*(.+)` 逐行提取, 行数必须等于
/// `expected_count` 才返回, 否则返回 `None` (调用方回退原文)。
pub fn parse_lines(input: &str, expected_count: usize) -> Option<Vec<String>> {
    let mut texts: Vec<String> = Vec::with_capacity(expected_count);
    for line in input.trim().split('\n') {
        if let Some(m) = line.match_ocr_line() {
            texts.push(m);
        }
    }
    if texts.len() == expected_count {
        Some(texts)
    } else {
        None
    }
}

/// 把 "1: 文本" / "1) 文本" / "1. 文本" 的行提取为文本, 否则返回 None。
trait OcrLine {
    fn match_ocr_line(&self) -> Option<String>;
}

impl OcrLine for str {
    fn match_ocr_line(&self) -> Option<String> {
        let t = self.trim();
        let mut chars = t.char_indices();
        // 前导数字
        let mut end = None;
        for (i, c) in &mut chars {
            if c.is_ascii_digit() {
                end = Some(i + c.len_utf8());
            } else {
                break;
            }
        }
        let end = end?;
        let rest = &t[end..];
        let sep = rest.trim_start_matches([' ', '\t']);
        // 分隔符须为 ):.
        let first = sep.chars().next()?;
        if !matches!(first, ')' | ':' | '.') {
            return None;
        }
        let text = sep[first.len_utf8()..].trim();
        if text.is_empty() {
            return None;
        }
        Some(text.to_string())
    }
}

/// 构造系统提示 (镜像 TS `buildOcrFixSystemPrompt`)。
///
/// `lang_label` 为语言展示名 (如 "中文"), `domain_hint` 可选领域提示。
pub fn build_ocr_fix_system_prompt(lang_label: &str, domain_hint: Option<&str>) -> String {
    let mut prompt = format!(
        "你是一个 OCR 纠错助手。修正{} OCR 文本中的错误。\n\n\
输入包含两部分：\n\
1. \"全文上下文\" — 完整对话，帮助理解语境\n\
2. \"请修正以下条目\" — 按行号列出的待修正文本\n\n\
OCR 常见错误类型：\n\
- 形近字混淆（如：方一→万一、想千什么→想干什么）\n\
- 单字幻觉（OCR 偶然多识别出一个字，如\"凭什么公给你看\" → \"凭什么给你看\"）\n\
- 标点缺失（字幕原有标点被 OCR 吞掉，根据上下语境合理补充）\n\n\
规则：\n\
1. 先参考全文上下文理解语境，再逐条修正\n\
2. 保持行号不变\n\
3. 只修改文字错误\n\
4. 保持行数完全一致\n\
5. 不要添加解释或额外内容\n\
6. 没有错误的行保持原样\n\
7. 注意：OCR 常见形近字而非同音字错误",
        lang_label
    );
    if let Some(hint) = domain_hint {
        prompt.push_str(&format!("\n\n领域提示：{hint}"));
    }
    prompt
}

/// 由 segments 构造用户 prompt (镜像 TS `ocrSegmentsToPrompt`)。
pub fn ocr_segments_to_prompt(segments: &[String]) -> String {
    let full_text = segments.join(" ");
    let lines: Vec<String> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}: {}", i + 1, s))
        .collect();
    format!(
        "全文上下文（参考用，每句以空格分隔）：\n{full_text}\n\n请修正以下条目（保持行号不变）：\n{}",
        lines.join("\n")
    )
}

/// 调 LLM 修正 OCR 段文本, 返回与输入等长、顺序对应的修正文本。
///
/// 镜像 TS `ocrLlmFix`: 失败 (解析行数不符) 时抛错, 由调用方决定回退原文还是中断。
/// `lang_label` 为语言展示名; `segments` 为待修正文本列表。
pub fn ocr_llm_fix(
    segments: &[String],
    lang_label: &str,
    args: &LlmFixArgs,
) -> anyhow::Result<Vec<String>> {
    let prompt = ocr_segments_to_prompt(segments);
    let system = build_ocr_fix_system_prompt(lang_label, args.domain_hint.as_deref());
    let opts = crate::ChatOptions {
        model: Some(args.llm_model.clone()),
        api_base: Some(args.llm_api_base.clone()),
        system_prompt: system,
        api_key: None,
        max_tokens: Some(4096),
        temperature: Some(0.1),
    };
    let fixed = crate::chat_completions(&prompt, &opts)?;
    parse_lines(&fixed, segments.len())
        .ok_or_else(|| anyhow!("LLM 回复行数不匹配 (期望 {} 行)", segments.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines_matches_numbered() {
        let inp = "1: 万一件事情\n2) 凭什么给你看\n3. 想干什么";
        let got = parse_lines(inp, 3).unwrap();
        assert_eq!(got, vec!["万一件事情", "凭什么给你看", "想干什么"]);
    }

    #[test]
    fn parse_lines_rejects_wrong_count() {
        let inp = "1: a\n2: b";
        assert!(parse_lines(inp, 3).is_none());
        assert!(parse_lines(inp, 2).is_some());
    }

    #[test]
    fn parse_lines_ignores_unnumbered() {
        // 无行号的行被忽略, 导致数量不足 -> None
        let inp = "万一件事情\n凭什么给你看";
        assert!(parse_lines(inp, 2).is_none());
    }

    #[test]
    fn system_prompt_includes_domain_hint() {
        let p = build_ocr_fix_system_prompt("中文", Some("仙侠题材"));
        assert!(p.contains("仙侠题材"));
        assert!(p.contains("中文"));
        let p2 = build_ocr_fix_system_prompt("中文", None);
        assert!(!p2.contains("领域提示"));
    }

    #[test]
    fn segments_to_prompt_has_full_and_lines() {
        let p = ocr_segments_to_prompt(&["a".into(), "b".into()]);
        assert!(p.contains("全文上下文"));
        assert!(p.contains("1: a"));
        assert!(p.contains("2: b"));
    }
}
