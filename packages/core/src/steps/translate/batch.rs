//! 批量翻译: 用 tool loop 让模型把译文作为 tool call 的 arguments 输出, 再归一化成
//! `{"1":"译1",...}`。
//!
//! 架构（见 `packages/llm` 的 `chat_with_tool_loop` + `TranslationToolHandler`）：
//! - 模型被要求调用 `submit` tool，arguments 是逐句译文。
//! - handler 用归一化器把 provider 自造结构（agnes 实测 argument/arguments/content/
//!   param1/编号行等）解析成标准 `{"1":...}`；失败把 feedback 回传，模型在同上下文重试。
//! - 归一化成功即 tool loop 终止（透穿结果）；外层再逐句 `check_sentence` 校验质量。
//! - 重试耗尽仍有缺失则返回 [`TranslateBatchError`] (携带已译部分), 由调用方写 partial。

use crate::steps::translate::parse_json_reply;

/// 重试次数 (镜像 TS 的 3 次)。
pub const MAX_ATTEMPTS: usize = 3;

/// 目标非中文时, 译文中汉字占比超过这个值即视为失败 (LLM 偷懒没翻译)。
pub const MAX_CHINESE_RATIO: f64 = 0.3;

/// 批量翻译失败但携带已译部分的错误 (供调用方写 partial + 暴露缺失)。
#[derive(Debug)]
pub struct TranslateBatchError {
    pub message: String,
    /// 与 batch 等长的填充, 已译句为 Some, 缺失为 None。
    ///
    /// **缺失信息只由 `filled` 的 `None` 表达**——以前还有个 `missing: Vec<usize>`
    /// 索引表, 但它从未被读取 (调用方只用 `filled`), 是同一信息的重复编码, 已删。
    pub filled: Vec<Option<String>>,
}

/// 统计字符串中汉字占比 (镜像 TS chineseRatio)。
fn chinese_ratio(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let han = s
        .chars()
        .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
        .count();
    han as f64 / s.chars().count() as f64
}

/// 校验单句译文是否可接受; 返回 `Err(原因)` 表示这句要重翻。
///
/// 两条规则 (与 TS 对齐):
/// - **目标非中文**时汉字占比 > [`MAX_CHINESE_RATIO`] → 判定 LLM 没翻译
///   (目标就是中文时这条不适用, 否则正常的中文译文会被自己否掉)
/// - 译文为空 → 失败
///
/// 这是**纯函数**, 不碰网络/文件, 可以单测 (见下方 tests)。
fn check_sentence(text: &str, index: usize, target_lang: &str) -> Result<(), String> {
    if target_lang != "zh" && chinese_ratio(text) > MAX_CHINESE_RATIO {
        return Err(format!(
            "第 {} 句仍含中文 (ratio={:.2}, 期望 {})",
            index + 1,
            chinese_ratio(text),
            target_lang
        ));
    }
    if text.is_empty() {
        return Err(format!("第 {} 句译文为空", index + 1));
    }
    Ok(())
}

/// 把"缺哪几句"格式化成可读列表 (供日志与错误信息)。
///
/// `reasons`: 每句自己的失败原因 (按原始索引); 取不到时用 `fallback` (批次级
/// 原因, 如 LLM 调用失败 / 解析失败)。以前用一个 `last_err` 给所有句套同一条,
/// 结果整批空译文时 25 句都报「第 25 句译文为空」——原因属于最后一句却被
/// 复制给全部, 误导排查。
/// `cap` 控制原文截断长度——日志里短一点, 错误信息里长一点。
fn describe_missing(
    batch: &[String],
    pending: &[usize],
    reasons: &std::collections::HashMap<usize, String>,
    fallback: &str,
    cap: usize,
) -> Vec<String> {
    pending
        .iter()
        .map(|&i| {
            let src: String = batch[i].chars().take(cap).collect();
            let ellipsis = if batch[i].chars().count() > cap {
                "…"
            } else {
                ""
            };
            let r = reasons.get(&i).map(|s| s.as_str()).unwrap_or(fallback);
            if r.is_empty() {
                format!("#{} (原文: {}{})", i + 1, src, ellipsis)
            } else {
                format!("#{} (原因: {}; 原文: {}{})", i + 1, r, src, ellipsis)
            }
        })
        .collect()
}

/// 批量翻译 (含 [`MAX_ATTEMPTS`] 轮外层重试), 返回与 `batch` 等长的译文。
///
/// 每轮：tool loop 让模型输出译文 → 归一化成 `{"1":...}` → 逐句质量校验。
/// tool loop 内部负责"输出形状收敛"（归一化失败 feedback 回传重试），
/// 外层负责"译文质量收敛"（含中文/空句在下一轮重来）。
pub async fn translate_batch(
    batch: &[String],
    system: &str,
    target_lang: &str,
    api_base: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<Vec<String>, TranslateBatchError> {
    let numbered: String = batch
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    let opts = llm::ChatOptions {
        model: Some(model.to_string()),
        api_base: Some(api_base.to_string()),
        system_prompt: system.to_string(),
        api_key: api_key.map(str::to_string),
        max_tokens: None,
        temperature: Some(0.2),
        tools: None,
        tool_choice: None,
    };
    let handler = llm::TranslationToolHandler::new(batch.len());

    // filled[i] 为已成功翻译的句子 (None = 待翻译)
    let mut filled: Vec<Option<String>> = vec![None; batch.len()];
    let mut batch_err = "未知原因 (LLM 未返回可用结果)".to_string();
    let mut reasons: std::collections::HashMap<usize, String> = std::collections::HashMap::new();

    for attempt in 0..MAX_ATTEMPTS {
        reasons.clear();

        // tool loop: 归一化成功即终止，返回标准 JSON 字符串 {"1":"译1",...}。
        let tool_output = match llm::chat_with_tool_loop(
            system,
            &numbered,
            &handler,
            &opts,
            MAX_ATTEMPTS,
        ).await {
            Ok(out) => out,
            Err(e) => {
                batch_err = format!("tool loop 失败: {e}");
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, batch_err);
                continue;
            }
        };

        // 归一化结果解析成序号→译文（handler 已保证是 {"1":...} 标准形态）。
        let parsed: serde_json::Value = match parse_json_reply(&tool_output) {
            Ok(v) => v,
            Err(e) => {
                batch_err = format!("归一化结果解析失败: {e}; output={tool_output}");
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, batch_err);
                continue;
            }
        };

        let mut still_pending: Vec<usize> = Vec::new();
        for i in 0..batch.len() {
            let key = (i + 1).to_string();
            let d = parsed.get(&key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            match check_sentence(&d, i, target_lang) {
                Ok(()) => {
                    filled[i] = Some(d);
                    reasons.remove(&i);
                }
                Err(reason) => {
                    reasons.insert(i, reason);
                    still_pending.push(i);
                }
            }
        }

        if still_pending.is_empty() {
            return Ok(filled.into_iter().map(|o| o.unwrap_or_default()).collect());
        }

        let pending_detail = still_pending.iter().map(|&i| i + 1).collect::<Vec<_>>();
        tracing::warn!(
            target: "translate",
            "batch attempt {} 后仍有 {} 句未通过质量校验: {:?}",
            attempt + 1,
            still_pending.len(),
            pending_detail
        );
    }

    // 重试耗尽仍有缺失: 返回携带已译部分的错误, 不兜底
    let pending: Vec<usize> = (0..batch.len()).filter(|&i| filled[i].is_none()).collect();
    if !pending.is_empty() {
        let missing_detail = describe_missing(batch, &pending, &reasons, &batch_err, 60);
        return Err(TranslateBatchError {
            message: format!(
                "批量翻译 {} 轮后仍缺失 {} 句 (期望 {}/{} 句, 期望语言 {}): {}",
                MAX_ATTEMPTS,
                pending.len(),
                batch.len() - pending.len(),
                batch.len(),
                target_lang,
                missing_detail.join("; ")
            ),
            filled,
        });
    }

    Ok(filled.into_iter().map(|o| o.unwrap_or_default()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_ratio_detects_cjk() {
        assert!(chinese_ratio("你好world") > 0.0);
        assert_eq!(chinese_ratio("hello"), 0.0);
        assert_eq!(chinese_ratio(""), 0.0);
        // 全中文 → 1.0
        assert_eq!(chinese_ratio("你好"), 1.0);
    }

    /// 目标非中文时, 汉字占比过高 = LLM 没翻译。
    #[test]
    fn chinese_translation_fails_when_target_is_not_chinese() {
        // 全中文 → ratio 1.0 > 0.3
        assert!(check_sentence("你好世界", 0, "vi").is_err());
        // 中英混排但中文占 2/4 = 0.5 > 0.3
        assert!(check_sentence("你好ab", 0, "vi").is_err());
        // 中文占 2/10 = 0.2 < 0.3 → 通过
        assert!(check_sentence("你好abcdefgh", 0, "vi").is_ok());
    }

    /// **目标就是中文时不做占比检查**——否则正常中文译文会被自己否掉。
    #[test]
    fn chinese_ratio_is_ignored_when_target_is_chinese() {
        assert!(check_sentence("你好世界", 0, "zh").is_ok());
        assert!(check_sentence("纯中文也可以", 3, "zh").is_ok());
    }

    /// 空译文永远失败, 与目标语言无关。
    #[test]
    fn empty_translation_always_fails() {
        for lang in ["zh", "vi", "en"] {
            let err = check_sentence("", 0, lang).expect_err("空译文必须失败");
            assert!(err.contains("第 1 句译文为空"), "实际: {err}");
        }
        // 只有空白也算空 (调用前已 trim)
        assert!(check_sentence("", 7, "vi").unwrap_err().contains("第 8 句"));
    }

    /// 失败原因里带 1-based 句号 (对齐 TS 的 `第 N 句`), 便于定位。
    #[test]
    fn failure_reason_uses_one_based_index() {
        let err = check_sentence("", 41, "vi").unwrap_err();
        assert!(err.contains("第 42 句"), "实际: {err}");
    }

    /// 缺失描述: 每句自己的原因 + fallback + 长原文截断。
    #[test]
    fn describe_missing_uses_per_sentence_reasons() {
        let batch: Vec<String> = vec!["short".into(), "x".repeat(100)];
        let pending = vec![0usize, 1];

        // 无原因: 只报原文
        let reasons = std::collections::HashMap::new();
        let plain = describe_missing(&batch, &pending, &reasons, "", 40);
        assert_eq!(plain.len(), 2);
        assert!(plain[0].starts_with("#1 (原文: short"), "实际: {}", plain[0]);
        assert!(plain[1].contains('…'), "超长原文应截断: {}", plain[1]);
        assert!(!plain[0].contains("原因"), "不带原因时不应出现: {}", plain[0]);

        // 每句独立原因: 第 1 句有自己的, 第 2 句没有 → 用 fallback。
        let mut reasons = std::collections::HashMap::new();
        reasons.insert(0usize, "空译文".to_string());
        let with_reason = describe_missing(&batch, &pending, &reasons, "批次解析失败", 60);
        assert!(with_reason[0].contains("原因: 空译文"), "实际: {}", with_reason[0]);
        assert!(with_reason[1].contains("原因: 批次解析失败"), "实际: {}", with_reason[1]);
        // cap 更大 → 截断位置更靠后
        assert!(
            with_reason[1].len() > plain[1].len(),
            "cap 60 应比 cap 40 保留更多原文"
        );
    }

    /// 常量是行为的一部分: 改了重试次数/占比阈值会改变翻译质量与耗时, 钉住防止误改。
    #[test]
    fn tuning_constants_are_pinned() {
        assert_eq!(MAX_ATTEMPTS, 3);
        assert_eq!(MAX_CHINESE_RATIO, 0.3);
    }
}
