//! sf_ocr_fix 的 LLM 修正 glue (镜像 TS `packages/core/ml/llm/ocr_llm_fix.ts` +
//! `srt_shared.ts::parseLines`)。
//!
//! 仅含纯函数: 构造 prompt / 解析 LLM 回复 / 调 [`crate::chat_completions`]。不含任何二进制
//! 调用或 IO, 方便单元测试。

use crate::LlmFixArgs;
use anyhow::anyhow;

/// 解析 LLM 的**压缩格式**修正回复：只输出有改动的行号（"2: 修正文本"），未出现的
/// 行表示"无误、保持原文"。
///
/// 返回与 `originals` 等长的列表：出现的 id 覆盖对应项，其余保持原文。
/// 模型即使输出全部行（密集）也照样工作。仅在以下情况报错：
/// - id 越界（不在 1..=len）
/// - id 重复
/// - 一行都解析不出（格式全错，避免把"模型完全没照格式"误当成"全部无误"）
pub fn parse_fix(input: &str, originals: &[String]) -> anyhow::Result<Vec<String>> {
    let mut fixed = originals.to_vec();
    let mut seen = vec![false; originals.len()];
    let mut any = false;
    for line in input.trim().split('\n') {
        let Some((id, text)) = line.match_ocr_line() else {
            continue;
        };
        if id == 0 || id > originals.len() {
            return Err(anyhow!("OCR 修正 id 越界: {id} (有效 1..={})", originals.len()));
        }
        if seen[id - 1] {
            return Err(anyhow!("OCR 修正 id 重复: {id}"));
        }
        seen[id - 1] = true;
        fixed[id - 1] = text;
        any = true;
    }
    if !any {
        return Err(anyhow!("OCR 修正回复无有效行 (需至少一行 N: 文本)"));
    }
    Ok(fixed)
}

/// 密集解析（供 ASR 修正用）：要求出现的行恰好覆盖 1..=expected_count 且顺序对应。
/// 与 `parse_fix` 接纳压缩格式不同——ASR 修正的调用方按位置 zip 回填，必须等长。
pub fn parse_lines(input: &str, expected_count: usize) -> Option<Vec<String>> {
    let placeholders = (0..expected_count).map(|_| String::new()).collect::<Vec<_>>();
    parse_fix(input, &placeholders).ok().filter(|v| {
        // 密集：所有 id 都必须出现（parse_fix 把未出现的留成空串）。
        v.iter().all(|s| !s.is_empty())
    })
}

/// 把 "1: 文本" / "1) 文本" / "1. 文本" 的行提取为 `(行号, 文本)`, 否则返回 None。
trait OcrLine {
    fn match_ocr_line(&self) -> Option<(usize, String)>;
}

impl OcrLine for str {
    fn match_ocr_line(&self) -> Option<(usize, String)> {
        let t = self.trim();
        let mut chars = t.char_indices();
        let mut end = None;
        for (i, c) in &mut chars {
            if c.is_ascii_digit() {
                end = Some(i + c.len_utf8());
            } else {
                break;
            }
        }
        let end = end?;
        let id: usize = t[..end].parse().ok()?;
        let rest = &t[end..];
        let sep = rest.trim_start_matches([' ', '\t']);
        let first = sep.chars().next()?;
        if !matches!(first, ')' | ':' | '.') {
            return None;
        }
        let text = sep[first.len_utf8()..].trim();
        if text.is_empty() {
            return None;
        }
        Some((id, text.to_string()))
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
pub async fn ocr_llm_fix(
    segments: &[String],
    lang_label: &str,
    args: &LlmFixArgs,
    model: &str,
    api_base: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let prompt = ocr_segments_to_prompt(segments);
    let system = build_ocr_fix_system_prompt(lang_label, args.domain_hint.as_deref());
    let opts = crate::ChatOptions {
        model: Some(model.to_string()),
        api_base: Some(api_base.to_string()),
        system_prompt: system,
        api_key: api_key.map(str::to_string),
        // 不设 max_tokens：让服务端按模型默认上限生成，避免固定 4096 截断长回复。
        max_tokens: None,
        temperature: Some(0.1),
        tools: None,
        tool_choice: None,
    };
    let result = crate::chat_completions_detailed(&prompt, &opts).await?;
    let fixed = result.content;
    parse_fix(&fixed, segments).map_err(|e| {
        // 诊断：失败时把原始回复与 finish_reason 打出来，否则只知道解析失败，
        // 无法区分 id 越界/重复、格式全错、还是被长度截断（finish_reason=length）。
        let preview: String = fixed.chars().take(2000).collect();
        tracing::warn!(target: "llm",
            "OCR 修正解析失败: {}, finish_reason={:?}, 原始回复(前2000字符)=\n{}",
            e, result.finish_reason, preview
        );
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origs(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("原文{i}")).collect()
    }

    #[test]
    fn parse_fix_dense_matches_numbered() {
        let got = parse_fix("1: 万一件事情\n2) 凭什么给你看\n3. 想干什么", &origs(3)).unwrap();
        assert_eq!(got, vec!["万一件事情", "凭什么给你看", "想干什么"]);
    }

    #[test]
    fn parse_fix_compressed_keeps_original_for_missing_ids() {
        // 模型用压缩格式只输出有改动的行 (2/9)，其余保持原文。
        let originals = origs(3);
        let got = parse_fix("2: 修正二", &originals).unwrap();
        assert_eq!(got, vec!["原文1", "修正二", "原文3"]);
    }

    #[test]
    fn parse_fix_ignores_unnumbered_lines() {
        let got = parse_fix("以下是修正：\n2: 修正二\n谢谢", &origs(3)).unwrap();
        assert_eq!(got, vec!["原文1", "修正二", "原文3"]);
    }

    #[test]
    fn parse_fix_rejects_out_of_range_and_duplicate_ids() {
        assert!(parse_fix("4: 越界", &origs(3)).is_err());
        assert!(parse_fix("2: a\n2: b", &origs(3)).is_err());
        assert!(parse_fix("0: 零", &origs(3)).is_err());
    }

    #[test]
    fn parse_fix_rejects_no_valid_lines() {
        // 全是无编号行 → 不能当成"全部无误"，应报错让调用方回退原文。
        assert!(parse_fix("万一件事情\n凭什么给你看", &origs(2)).is_err());
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
