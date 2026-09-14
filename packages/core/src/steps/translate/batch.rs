//! 批量翻译: 一次 LLM 调用翻一批句子, 失败按**子集**重试 (镜像 TS `translateBatch`)。
//!
//! 重试策略: 若某次返回的译文数量与 batch 不符, 已匹配的句直接保留, 后续 attempt
//! 只对**缺失的句子**重新请求 —— 避免像 49/50 这种情况因为一句话抽风而整批重来。
//! 但**不**无脑兜底: 重试耗尽仍有缺失则返回 [`TranslateBatchError`] (携带已译部分),
//! 交由调用方写 partial 落盘并暴露缺失, 而不会把原文当译文塞回去。

use serde_json::Value;

use crate::steps::translate::args;
use crate::steps::translate::parse_json_reply;
use crate::steps::utils::lang_name;
use config_rs::env::openai_api_key;

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
/// `reason` 为 `Some` 时每条附上最后一次失败原因 (终局错误用它, 逐轮 warn 不带)。
/// `cap` 控制原文截断长度——日志里短一点, 错误信息里长一点。
///
/// 以前这段在 `translate_batch` 里有**两份几乎相同的拷贝**, 只差截断长度与
/// 是否带原因; 合成一个函数后不会再漏改其中一份。
fn describe_missing(
    batch: &[String],
    pending: &[usize],
    reason: Option<&str>,
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
            match reason {
                Some(r) => format!("#{} (原因: {}; 原文: {}{})", i + 1, r, src, ellipsis),
                None => format!("#{} (原文: {}{})", i + 1, src, ellipsis),
            }
        })
        .collect()
}

/// 批量翻译 (含 [`MAX_ATTEMPTS`] 次重试), 返回与 `batch` 等长的译文。
pub fn translate_batch(
    batch: &[String],
    system: &str,
    target_lang: &str,
    args: &args::TranslateArgs,
) -> Result<Vec<String>, TranslateBatchError> {
    let numbered: String = batch
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    // filled[i] 为已成功翻译的句子 (None = 待翻译)
    let mut filled: Vec<Option<String>> = vec![None; batch.len()];
    let mut pending: Vec<usize> = (0..batch.len()).collect();
    let mut last_err = "未知原因 (LLM 未返回可用结果)".to_string();

    for attempt in 0..MAX_ATTEMPTS {
        // 构造本轮待翻译子集的编号文本
        let subset_numbered: String = pending
            .iter()
            .map(|&i| format!("{}. {}", i + 1, batch[i]))
            .collect::<Vec<_>>()
            .join("\n");
        let user_msg = if attempt > 0 {
            format!(
                "{subset_numbered}\n\n（注意：以上回复包含中文！必须全部输出{lang}译文，不得包含任何中文。）",
                subset_numbered = subset_numbered,
                lang = lang_name(target_lang)
            )
        } else if pending.len() == batch.len() {
            // 首轮用全量编号, 后续轮用子集编号
            numbered.clone()
        } else {
            subset_numbered
        };

        let reply = match llm::chat_completions(
            &user_msg,
            &llm::ChatOptions {
                model: Some(args.model.clone()),
                api_base: Some(args.api_base.clone()),
                system_prompt: system.to_string(),
                api_key: openai_api_key(),
                max_tokens: Some(3072),
                temperature: Some(0.2),
            },
        ) {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("LLM 调用失败: {e}");
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, last_err);
                continue;
            }
        };
        let parsed: Value = match parse_json_reply(&reply) {
            Ok(v) => v,
            Err(e) => {
                last_err = format!(
                    "回复无法解析为 JSON: {e}; 原始回复前 200 字符: {}",
                    &reply[..reply.len().min(200)]
                );
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, last_err);
                continue;
            }
        };
        let arr = match parsed.get("dst").and_then(|d| d.as_array()) {
            Some(a) => a,
            None => {
                last_err = "LLM 回复缺少 dst 数组".to_string();
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}; 原始回复: {}", attempt + 1, last_err, &reply[..reply.len().min(200)]);
                if attempt == MAX_ATTEMPTS - 1 {
                    break;
                }
                continue;
            }
        };

        // 将本轮回复按 pending 顺序对齐填入 filled
        let mut still_pending: Vec<usize> = Vec::with_capacity(pending.len());
        for (slot, &i) in pending.iter().enumerate() {
            let d = arr
                .get(slot)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            match check_sentence(&d, i, target_lang) {
                Ok(()) => filled[i] = Some(d),
                Err(reason) => {
                    last_err = reason;
                    still_pending.push(i);
                }
            }
        }
        pending = still_pending;

        if pending.is_empty() {
            // 全部补齐
            return Ok(filled
                .iter()
                .map(|o| o.clone().unwrap_or_default())
                .collect());
        }

        // 暴露具体缺失哪句 + 原文, 供评估而非无脑兜底
        let missing_detail = describe_missing(batch, &pending, None, 40);
        tracing::warn!(
            target: "translate",
            "batch attempt {} 后仍有 {} 句未翻译: {}",
            attempt + 1,
            pending.len(),
            missing_detail.join("; ")
        );
        if attempt == MAX_ATTEMPTS - 1 {
            break;
        }
    }

    // 重试耗尽仍有缺失: 返回携带已译部分的错误 (filled 已含 Some/None), 不兜底
    if !pending.is_empty() {
        let missing_detail = describe_missing(batch, &pending, Some(&last_err), 60);
        return Err(TranslateBatchError {
            message: format!(
                "批量翻译 {} 次重试后仍缺失 {} 句 (期望 {}/{} 句, 期望语言 {}): {}",
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
        assert!(check_sentence("", 7, "vi")
            .unwrap_err()
            .contains("第 8 句"));
    }

    /// 失败原因里带 1-based 句号 (对齐 TS 的 `第 N 句`), 便于定位。
    #[test]
    fn failure_reason_uses_one_based_index() {
        let err = check_sentence("", 41, "vi").unwrap_err();
        assert!(err.contains("第 42 句"), "实际: {err}");
    }

    /// 缺失描述: 带/不带原因两种形态 + 长原文截断。
    #[test]
    fn describe_missing_formats_both_variants() {
        let batch: Vec<String> = vec!["short".into(), "x".repeat(100)];
        let pending = vec![0usize, 1];

        let plain = describe_missing(&batch, &pending, None, 40);
        assert_eq!(plain.len(), 2);
        assert!(plain[0].starts_with("#1 (原文: short"), "实际: {}", plain[0]);
        assert!(plain[1].contains('…'), "超长原文应截断: {}", plain[1]);
        assert!(!plain[0].contains("原因"), "不带原因时不应出现: {}", plain[0]);

        let with_reason = describe_missing(&batch, &pending, Some("超时"), 60);
        assert!(with_reason[0].contains("原因: 超时"), "实际: {}", with_reason[0]);
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
