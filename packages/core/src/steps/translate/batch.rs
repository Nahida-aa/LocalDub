//! 批量翻译: 一次 LLM 调用翻一批句子, 失败按**子集**重试 (镜像 TS `translateBatch`)。
//!
//! 重试策略: 若某次返回的译文数量与 batch 不符, 已匹配的句直接保留, 后续 attempt
//! 只对**缺失的句子**重新请求 —— 避免像 49/50 这种情况因为一句话抽风而整批重来。
//! 但**不**无脑兜底: 重试耗尽仍有缺失则返回 [`TranslateBatchError`] (携带已译部分),
//! 交由调用方写 partial 落盘并暴露缺失, 而不会把原文当译文塞回去。

use serde_json::{json, Value};

use crate::steps::translate::parse_json_reply;
use crate::steps::utils::lang_name;

/// 构造本轮 function calling 的 tool 定义：key 为句子序号（字符串），value 为译文。
///
/// 序号用**原始 1-based**（`i + 1`），与 `pending` 里的原始索引一致，重试子集也用
/// 原序号。`required` 列全 + `additionalProperties:false` 是给"遵守 schema"的
/// provider 的强提示——但 **agnes 实测不遵守**（同一批返回过 `param1..25` /
/// `{"argument":"<json>"}` / `{"parameter_name":"1","value":..}`），所以解析侧
/// 另有 [`collect_translations`] 多层兜底，不依赖 key 一定是 `"1".."N"`。
fn build_translate_tool(pending: &[usize]) -> Vec<Value> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::with_capacity(pending.len());
    for &i in pending {
        let key = (i + 1).to_string();
        properties.insert(key.clone(), json!({ "type": "string" }));
        required.push(Value::String(key));
    }
    vec![json!({
        "type": "function",
        "function": {
            "name": "emit_translation",
            "description": "逐句翻译结果。key 为句子序号（字符串），value 为对应译文。全员必填。",
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }
        }
    })]
}

/// 从模型返回的 JSON 里按序号捞译文，不假设 key 一定是 `"1".."N"`。
///
/// 背景（2026-11-26，agnes provider）：tool schema 要求 key 为 `"1".."25"`，
/// 但同一批连续三次返回三种畸形结构——
/// 1. `{"param1":"译1", ..., "param25":"译25"}` — 自造 key 名，但尾数字是序号
/// 2. `{"argument":"{\"1\":\"译1\", ...}"}` — 整包塞进一个字符串（双层 JSON）
/// 3. `{"parameter_name":"1","value":"译1"}` — 单条 kv 对象
///
/// 策略（按可信度）：
/// - 对象 key 是纯数字串 → 直接当序号（理想 schema）
/// - 对象 key 带数字尾巴（`param7`/`seg_12`）→ 取尾数字当序号
/// - value 本身是 JSON 字符串 → 解开递归（应对双层包裹）
/// - 数组 → 位置即序号（1-based）
///
/// 只收 `pending` 里的序号，别的忽略。序号统一按 `i + 1`（1-based）。
fn collect_translations(v: &Value, pending: &[usize], out: &mut std::collections::HashMap<usize, String>) {
    let wanted: std::collections::HashSet<usize> = pending.iter().map(|&i| i + 1).collect();
    fn digits(s: &str) -> Option<usize> {
        let tail: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
        if tail.is_empty() {
            None
        } else {
            tail.chars().rev().collect::<String>().parse().ok()
        }
    }
    fn walk(v: &Value, wanted: &std::collections::HashSet<usize>, out: &mut std::collections::HashMap<usize, String>) {
        match v {
            Value::Object(m) => {
                for (k, val) in m {
                    match val {
                        Value::String(s) => {
                            if let Some(n) = digits(k) {
                                if wanted.contains(&n) {
                                    out.entry(n).or_insert_with(|| s.trim().to_string());
                                }
                            } else if s.trim_start().starts_with('{') || s.trim_start().starts_with('[') {
                                // value 是 JSON 字符串 → 解开递归（双层包裹）
                                if let Ok(inner) = crate::steps::translate::parse_json_reply(s) {
                                    walk(&inner, wanted, out);
                                }
                            }
                        }
                        _ => walk(val, wanted, out),
                    }
                }
            }
            Value::Array(a) => {
                for (idx, val) in a.iter().enumerate() {
                    if let Some(s) = val.as_str() {
                        let n = idx + 1;
                        if wanted.contains(&n) {
                            out.entry(n).or_insert_with(|| s.trim().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    walk(v, &wanted, out);
}

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

/// 批量翻译 (含 [`MAX_ATTEMPTS`] 次重试), 返回与 `batch` 等长的译文。
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

    // filled[i] 为已成功翻译的句子 (None = 待翻译)
    let mut filled: Vec<Option<String>> = vec![None; batch.len()];
    let mut pending: Vec<usize> = (0..batch.len()).collect();
    let mut batch_err = "未知原因 (LLM 未返回可用结果)".to_string();
    // 每句独立的失败原因（终局错误按句展示，避免 last_err 互相覆盖）。
    let mut reasons: std::collections::HashMap<usize, String> = std::collections::HashMap::new();

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

        let completion = match llm::chat_completions_detailed(
            &user_msg,
            &llm::ChatOptions {
                model: Some(model.to_string()),
                api_base: Some(api_base.to_string()),
                system_prompt: system.to_string(),
                api_key: api_key.map(str::to_string),
                // 不设 max_tokens：让服务端按模型上限生成，避免固定上限截断。
                max_tokens: None,
                temperature: Some(0.2),
                tools: Some(build_translate_tool(&pending)),
                tool_choice: Some(json!({
                    "type": "function",
                    "function": { "name": "emit_translation" }
                })),
            },
        ).await {
            Ok(c) => c,
            Err(e) => {
                batch_err = format!("LLM 调用失败: {e}");
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, batch_err);
                continue;
            }
        };
        // 结构化输出走 tool_calls.arguments；provider 不支持 function calling 时退回正文。
        let reply = completion
            .tool_calls
            .first()
            .map(|tc| tc.function.arguments.clone())
            .unwrap_or_else(|| completion.content.clone());
        let finish_reason = completion.finish_reason.as_deref().unwrap_or("(none)");
        let parsed: Value = match parse_json_reply(&reply) {
            Ok(v) => v,
            Err(e) => {
                batch_err = format!(
                    "回复无法解析为 JSON: {e}; finish_reason={finish_reason}; 原始回复(前100/末尾100): {}",
                    crate::steps::utils::truncate_utf8_around(&reply, 100, 100)
                );
                tracing::warn!(target: "translate", "batch attempt {} 失败: {}", attempt + 1, batch_err);
                continue;
            }
        };

        // 诊断：模型实际返回了什么结构（顶层 key + 原文片段）。连续整批缺失时
        // 用来区分「schema 没被遵循」还是「key 被填成空串」还是「译文含中文被否」。
        let parsed_keys: Vec<&str> = parsed
            .as_object()
            .map(|m| m.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        tracing::info!(
            target: "translate",
            "batch attempt {} reply head={} parsed_keys={:?}",
            attempt + 1,
            crate::steps::utils::truncate_utf8(&reply, 300),
            parsed_keys
        );

        // 本轮成功解析 → 清掉上一轮遗留的逐句原因，只反映本轮结果。
        reasons.clear();

        // 从任意形状里按序号捞译文（agnes 不遵守 schema，实测三种畸形结构）。
        let mut extracted: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        collect_translations(&parsed, &pending, &mut extracted);
        let mut still_pending: Vec<usize> = Vec::with_capacity(pending.len());
        for &i in pending.iter() {
            let d = extracted.get(&(i + 1)).cloned().unwrap_or_default();
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
        pending = still_pending;

        // 每轮都记一次 finish_reason，便于区分 stop / length / tool_calls。
        tracing::info!(
            target: "translate",
            "batch attempt {} finish_reason={} tool_calls={} pending={}",
            attempt + 1,
            finish_reason,
            completion.tool_calls.len(),
            pending.len()
        );

        if pending.is_empty() {
            // 全部补齐
            return Ok(filled
                .iter()
                .map(|o| o.clone().unwrap_or_default())
                .collect());
        }

        // 暴露具体缺失哪句 + 原文, 供评估而非无脑兜底
        let missing_detail = describe_missing(batch, &pending, &reasons, "", 40);
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
        let missing_detail = describe_missing(batch, &pending, &reasons, &batch_err, 60);
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

    /// tool schema 的每个 key 是原始 1-based 序号，required 列全（schema 层面要求每句都输出）。
    #[test]
    fn build_translate_tool_keys_and_required_are_one_based() {
        let tools = build_translate_tool(&[0, 2, 4]);
        let f = &tools[0]["function"];
        assert_eq!(f["name"], "emit_translation");
        let props = f["parameters"]["properties"].as_object().unwrap();
        assert_eq!(props.len(), 3);
        assert!(props.contains_key("1"));
        assert!(props.contains_key("3"));
        assert!(props.contains_key("5"));
        assert!(!props.contains_key("2")); // 不在 pending 里
        let required = f["parameters"]["required"].as_array().unwrap();
        let keys: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(keys, vec!["1", "3", "5"]);
    }

    /// agnes 实测的三种畸形结构 + 理想 schema，都要能按序号捞到译文。
    #[test]
    fn collect_translations_handles_provider_shapes() {
        fn get(v: &Value, pending: &[usize]) -> Vec<Option<String>> {
            let mut out = std::collections::HashMap::new();
            collect_translations(v, pending, &mut out);
            pending.iter().map(|&i| out.get(&(i + 1)).cloned()).collect()
        }

        let pending = vec![0usize, 1, 2]; // 想要第 1/2/3 句

        // 理想 schema：纯数字 key
        let ideal = serde_json::json!({"1": "译1", "2": "译2", "3": "译3"});
        assert_eq!(
            get(&ideal, &pending),
            vec![Some("译1".into()), Some("译2".into()), Some("译3".into())]
        );

        // agnes 形态 1：自造 key 名 + 尾数字
        let param = serde_json::json!({"param1": "甲", "param2": "乙", "param3": "丙"});
        assert_eq!(
            get(&param, &pending),
            vec![Some("甲".into()), Some("乙".into()), Some("丙".into())]
        );

        // agnes 形态 2：整包塞进一个字符串（双层 JSON）
        let wrapped = serde_json::json!({"argument": "{\"1\":\"x\",\"2\":\"y\",\"3\":\"z\"}"});
        assert_eq!(
            get(&wrapped, &pending),
            vec![Some("x".into()), Some("y".into()), Some("z".into())]
        );

        // agnes 形态 3：单条 kv 对象（模型只回 1 句，且无可靠序号映射）→ 全 None，
        // 靠重试拿到形态 1/2 的完整结果，不硬解这种脆弱结构。
        let kv = serde_json::json!({"parameter_name": "2", "value": "只有第二句"});
        let got = get(&kv, &pending);
        assert!(got.iter().all(|x| x.is_none()));

        // 数组：位置即序号
        let arr = serde_json::json!(["A", "B", "C"]);
        assert_eq!(
            get(&arr, &pending),
            vec![Some("A".into()), Some("B".into()), Some("C".into())]
        );

        // 只收 pending 内的序号
        let extra = serde_json::json!({"1": "a", "9": "不该收"});
        let got = get(&extra, &pending);
        assert_eq!(got[0], Some("a".into()));
        assert!(got.iter().all(|x| x.as_deref() != Some("不该收")));
    }

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
