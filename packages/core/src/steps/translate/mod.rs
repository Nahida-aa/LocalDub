//! translate 阶段 (镜像 TS `packages/core/steps/05_translate/translate.ts`)。
//!
//! 从权威字幕文件拿时间轴 + 原文, 调 LLM 批量翻译成目标语言, 写出
//! `translate/translation.{lang}.json`。翻译是 dub/subtitle pipeline 共享的 step。
//!
//! 实现要点 (与 TS 对齐):
//! - 目标语言 input > auto 推断 (resolve_language), 写回 ctx.target_language
//! - 有 ytdlp_info.json 时先做 preprocess (summary/hotwords/corrections) 注入系统提示
//! - 批量 BATCH_SIZE=50, 每批 3 次重试; 目标非中文时校验译文中文占比 < 0.3
//! - 破折号 `——` 替换为逗号

pub mod args;
pub mod out;
pub mod partial;
pub mod prompts;

use std::path::Path;

use serde_json::Value;

use crate::context::WorkflowCtx;
use crate::steps::translate::out::{TranslateResult, TranslateResultMeta, TranslateSegment};
use crate::steps::translate::prompts::{build_preprocess_prompt, build_translate_system, MetaView};
use crate::steps::utils::{
    lang_name, now_iso, resolve_language, set_step_anyhow, subtitle_file_path,
    translation_file_path, translation_partial_path, StepPatch, StepStatus,
};
use config_rs::env::openai_api_key;

/// 从 `ctx.input.steps.translate` 解析配置 (镜像 TS `readInputArgs().steps.translate`)。
fn read_args(ctx: &WorkflowCtx) -> args::TranslateArgs {
    ctx.input
        .get("steps")
        .and_then(|v| v.get("translate"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 读 `download/ytdlp_info.json` → [`MetaView`]。
///
/// 文件缺失 / 内容非法都退化为 `MetaView::default()`——元信息只用于给 LLM
/// 加上下文, 缺了照样能翻, 不该让 step 失败。字段做了长度截断 (避免把整段
/// 视频简介塞进提示词)。
fn load_video_meta(path: &Path) -> MetaView {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return MetaView::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return MetaView::default();
    };
    let take = |key: &str, cap: usize| -> String {
        v.get(key)
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .chars()
            .take(cap)
            .collect()
    };
    let mut meta = MetaView {
        title: take("title", 500),
        uploader: take("uploader", 200),
        description: take("description", 500).replace("(none)", ""),
    };
    if meta.description.is_empty() {
        meta.description = "(none)".to_string();
    }
    meta
}

/// preprocess 的产物: 注入后续批量翻译系统提示的上下文。
#[derive(Debug, Default, Clone, PartialEq)]
struct PreprocessResult {
    summary: String,
    /// 热词表, 每行 `src -> dst`; 无则 `"(none)"`。
    hotwords: String,
    /// 纠错表, 每行 `wrong -> correct`; 无则 `"(none)"`。
    corrections: String,
}

impl PreprocessResult {
    fn hotwords_str(list: &[String]) -> String {
        if list.is_empty() {
            "(none)".to_string()
        } else {
            list.join("\n")
        }
    }
}

/// 让 LLM 先通读全文, 产出 summary / hotwords / corrections (镜像 TS 的 preprocess)。
///
/// **失败不阻断**: 网络/配额/解析任何一环出错, 都退化成 [`Default`]——
/// 预处理只是"翻得更好"的优化项, 不是翻得出来的前提。
fn run_preprocess(
    args: &args::TranslateArgs,
    src_lang_name: &str,
    dst_lang_name: &str,
    meta: &MetaView,
    full_text: &str,
) -> PreprocessResult {
    let prompt = build_preprocess_prompt(dst_lang_name, src_lang_name, meta, full_text);

    /// 从 `{"src":..,"dst":..}` 数组里拼 `a -> b` 行。
    fn pairs(v: &Value, key: &str, from: &str, to: &str) -> Vec<String> {
        v.get(key)
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| {
                        let a = x.get(from).and_then(|s| s.as_str())?;
                        let b = x.get(to).and_then(|s| s.as_str())?;
                        Some(format!("{a} -> {b}"))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    match llm::chat_completions(
        &prompt,
        &llm::ChatOptions {
            model: Some(args.model.clone()),
            api_base: Some(args.api_base.clone()),
            system_prompt: "You output strict JSON only.".to_string(),
            api_key: openai_api_key(),
            max_tokens: Some(2048),
            temperature: Some(0.2),
        },
    ) {
        Ok(raw) => match parse_json_reply(&raw) {
            Ok(v) => {
                let hotwords = pairs(&v, "hotwords", "src", "dst");
                let corrections = pairs(&v, "corrections", "wrong", "correct");
                PreprocessResult {
                    summary: v
                        .get("summary")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    hotwords: PreprocessResult::hotwords_str(&hotwords),
                    corrections: PreprocessResult::hotwords_str(&corrections),
                }
            }
            Err(e) => {
                tracing::warn!(target: "translate", "[Translate] Preprocess 回复解析失败: {e}");
                PreprocessResult::default()
            }
        },
        Err(e) => {
            tracing::warn!(target: "translate", "[Translate] Preprocess failed: {e}");
            PreprocessResult::default()
        }
    }
}

/// 解析 LLM 返回的 JSON: 优先整体解析, 否则提取首个 `{...}` 块。
fn parse_json_reply(raw: &str) -> anyhow::Result<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return Ok(v);
    }
    // 退回: 匹配首个 {...} (允许跨行)
    let start = raw.find('{');
    let end = raw.rfind('}');
    if let (Some(s), Some(e)) = (start, end) {
        if e > s {
            let sub = &raw[s..=e];
            return serde_json::from_str::<Value>(sub)
                .map_err(|e| anyhow::anyhow!("解析 LLM 回复 JSON 失败: {e}"));
        }
    }
    Err(anyhow::anyhow!(
        "无法从 LLM 回复解析 JSON: {}",
        &raw[..raw.len().min(300)]
    ))
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

/// 批量翻译失败但携带已译部分的错误 (供调用方写 partial + 暴露缺失)。
struct TranslateBatchError {
    message: String,
    /// 与 batch 等长的填充, 已译句为 Some, 缺失为 None。
    ///
    /// **缺失信息只由 `filled` 的 `None` 表达**——以前还有个 `missing: Vec<usize>`
    /// 索引表, 但它从未被读取 (调用方只用 `filled`), 是同一信息的重复编码, 已删。
    filled: Vec<Option<String>>,
}

/// 批量翻译 (含 3 次重试), 返回与 `batch` 等长的译文 (镜像 TS `translateBatch`)。
///
/// 重试策略: 若某次返回的译文数量与 batch 不符, 已匹配的句直接保留,
/// 后续 attempt 只对**缺失的句子**重新请求 (子集重试), 而非整批重来 ——
/// 避免像 49/50 这种情况因为一句话抽风而整批重来。但**不**无脑兜底:
/// 3 次重试后仍有缺失则返回 [`TranslateBatchError`] (携带已译部分与缺失索引),
/// 交由调用方写 partial 落盘并暴露缺失, 而不会把原文当译文塞回去。
fn translate_batch(
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

    for attempt in 0..3 {
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
        } else {
            // 首轮用全量编号, 后续轮用子集编号
            if attempt == 0 && pending.len() == batch.len() {
                numbered.clone()
            } else {
                subset_numbered
            }
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
        let parsed = match parse_json_reply(&reply) {
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
                if attempt == 2 {
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
            let bad = if target_lang != "zh" && chinese_ratio(&d) > 0.3 {
                last_err = format!(
                    "第 {} 句仍含中文 (ratio={:.2}, 期望 {})",
                    i + 1,
                    chinese_ratio(&d),
                    target_lang
                );
                true
            } else if d.is_empty() {
                last_err = format!("第 {} 句译文为空", i + 1);
                true
            } else {
                false
            };
            if bad {
                still_pending.push(i);
            } else {
                filled[i] = Some(d);
            }
        }
        pending = still_pending;

        if pending.is_empty() {
            // 全部补齐
            let out: Vec<String> = filled
                .iter()
                .map(|o| o.clone().unwrap_or_default())
                .collect();
            return Ok(out);
        }
        // 暴露具体缺失哪句 + 原文, 供评估而非无脑兜底
        let missing_detail: Vec<String> = pending
            .iter()
            .map(|&i| {
                let src = batch[i].chars().take(40).collect::<String>();
                format!(
                    "#{} (原文: {}{})",
                    i + 1,
                    src,
                    if batch[i].chars().count() > 40 {
                        "…"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        tracing::warn!(
            target: "translate",
            "batch attempt {} 后仍有 {} 句未翻译: {}",
            attempt + 1,
            pending.len(),
            missing_detail.join("; ")
        );
        if attempt == 2 {
            break;
        }
    }

    // 重试耗尽仍有缺失: 返回携带已译部分的错误 (filled 已含 Some/None), 不兜底
    if !pending.is_empty() {
        let missing_detail: Vec<String> = pending
            .iter()
            .map(|&i| {
                let src = batch[i].chars().take(60).collect::<String>();
                format!(
                    "#{} (原因: {}; 原文: {}{})",
                    i + 1,
                    last_err,
                    src,
                    if batch[i].chars().count() > 60 {
                        "…"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        return Err(TranslateBatchError {
            message: format!(
                "批量翻译 3 次重试后仍缺失 {} 句 (期望 {}/{} 句, 期望语言 {}): {}",
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

/// 入口 (镜像 TS `stepTranslate`)。
pub fn step_translate(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let video_dir = ctx.workflow.video_dir.clone();
    tracing::info!(target: "translate", "start");

    let args = read_args(ctx);
    let (src_lang, target_lang) = resolve_language(ctx)?;
    let src_lang_name = lang_name(&src_lang);
    let dst_lang_name = lang_name(&target_lang);

    let srt_file = subtitle_file_path(ctx);
    if !Path::new(&srt_file).exists() {
        return Err(anyhow::anyhow!(
            "字幕文件不存在: {srt_file}; 请先完成识别阶段"
        ));
    }
    let srt_raw = std::fs::read_to_string(&srt_file)
        .map_err(|e| anyhow::anyhow!("读取字幕文件 {srt_file} 失败: {e}"))?;
    let srt: Value = serde_json::from_str(&srt_raw)
        .map_err(|e| anyhow::anyhow!("解析字幕文件 {srt_file} 失败: {e}"))?;
    let srt_segments = srt
        .get("result")
        .and_then(|r| r.get("segments"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    if srt_segments.is_empty() {
        return Err(anyhow::anyhow!("{srt_file} 无 segments"));
    }
    let texts: Vec<String> = srt_segments
        .iter()
        .map(|u| {
            u.get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .collect();
    let full_text = srt
        .get("result")
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| texts.join(" "));

    // 视频元信息 (ytdlp_info.json 可选)
    let ytdlp_path = Path::new(&video_dir)
        .join("download")
        .join("ytdlp_info.json");
    let has_meta = ytdlp_path.exists();
    let meta = load_video_meta(&ytdlp_path);
    // preprocess (仅在有元信息时): 让 LLM 先读一遍全文, 产出摘要/热词/纠错,
    // 注入后续批量翻译的系统提示。失败不阻断——退化成"无预处理"直接翻。
    let pre = if has_meta {
        run_preprocess(&args, &src_lang_name, &dst_lang_name, &meta, &full_text)
    } else {
        PreprocessResult::default()
    };
    let summary = pre.summary;
    let hotwords_str = pre.hotwords;
    let corrections_str = pre.corrections;

    let system = build_translate_system(
        &dst_lang_name,
        &src_lang_name,
        &meta,
        &summary,
        &hotwords_str,
        &corrections_str,
    );

    let total_batches = texts.chunks(partial::BATCH_SIZE).count();
    let partial_path = translation_partial_path(&video_dir, &target_lang);

    // 阶段内续跑: 读已有 partial, 恢复已完成 batch 与已译句
    let (mut completed, partial_segs) = partial::read_partial(&partial_path);
    if !completed.is_empty() {
        tracing::info!(
            target: "translate",
            "resume: {} 个 batch 已完成, 跳过 (共 {} batch)",
            completed.len(),
            total_batches
        );
    }

    // 把 partial 中已完成 batch 的已译句回填到 dsts (缺失句 dst=None 仍待翻)
    let mut dsts: Vec<Option<String>> = vec![None; texts.len()];
    {
        let mut by_batch: std::collections::HashMap<usize, Vec<&out::TranslatePartialSegment>> =
            std::collections::HashMap::new();
        for ps in partial_segs.iter() {
            by_batch.entry(ps.batch_index).or_default().push(ps);
        }
        for (&bi, segs) in by_batch.iter() {
            if !completed.contains(&bi) {
                continue;
            }
            let start = bi * partial::BATCH_SIZE;
            for (k, ps) in segs.iter().enumerate() {
                let gi = start + k;
                if gi < dsts.len() && !ps.missing {
                    dsts[gi] = Some(ps.dst.clone());
                }
            }
        }
    }

    for (i, chunk) in texts.chunks(partial::BATCH_SIZE).enumerate() {
        if completed.contains(&i) {
            tracing::info!(target: "translate", "batch {}/{}: 已完成, 跳过", i + 1, total_batches);
            continue;
        }
        let batch: Vec<String> = chunk.to_vec();
        tracing::info!(target: "translate", "batch {}/{}: {} 句", i + 1, total_batches, batch.len());
        match translate_batch(&batch, &system, &target_lang, &args) {
            Ok(results) => {
                for (k, r) in results.into_iter().enumerate() {
                    let gi = i * partial::BATCH_SIZE + k;
                    if gi < dsts.len() {
                        dsts[gi] = Some(r.replace("——", "，"));
                    }
                }
                completed.insert(i);
            }
            Err(e) => {
                // 把已译部分写进 partial (含缺失标注), 然后暴露错误
                for (k, opt) in e.filled.into_iter().enumerate() {
                    let gi = i * partial::BATCH_SIZE + k;
                    if gi < dsts.len() {
                        dsts[gi] = opt.map(|s| s.replace("——", "，"));
                    }
                }
                partial::write_partial(
                    &partial_path,
                    &texts,
                    &srt_segments,
                    &dsts,
                    &completed,
                    &src_lang,
                    &target_lang,
                )
                .ok();
                return Err(anyhow::anyhow!("Step translate failed: {}", e.message));
            }
        }
        // 每 batch 完成即增量落盘 partial (阶段内续跑 + 分析用)
        partial::write_partial(
            &partial_path,
            &texts,
            &srt_segments,
            &dsts,
            &completed,
            &src_lang,
            &target_lang,
        )?;
        set_step_anyhow(
            &video_dir,
            "translate",
            StepPatch {
                last_message: Some(format!(
                    "Translating {}/{}...",
                    (i + 1) * partial::BATCH_SIZE,
                    texts.len()
                )),
                ..Default::default()
            },
        )
        .ok();
    }

    // 全部完成: 组装正式结果, 写 translation.{lang}.json, 删除 partial
    let segments: Vec<TranslateSegment> = srt_segments
        .iter()
        .enumerate()
        .map(|(idx, u)| TranslateSegment {
            text: texts[idx].clone(),
            dst: dsts.get(idx).cloned().flatten().unwrap_or_default(),
            src_lang: Some(src_lang.clone()),
            dst_lang: Some(target_lang.clone()),
            start_ms: u.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            end_ms: u.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            speaker: None,
        })
        .collect();

    let result = TranslateResult {
        segments,
        meta: TranslateResultMeta {
            src_lang: src_lang.clone(),
            target_lang: target_lang.clone(),
        },
    };

    let out_file = translation_file_path(&video_dir, &target_lang);
    if let Some(parent) = out_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("序列化翻译结果失败: {e}"))?;
    std::fs::write(&out_file, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", out_file.display(), e))?;

    // 正式结果已落盘, 删掉增量 partial (避免残留误导)
    let _ = std::fs::remove_file(&partial_path);

    // 确保 ctx.target_language 在内存之外也落盘 (resolve_language 已写回文件, 这里仅保险)
    set_step_anyhow(
        &video_dir,
        "translate",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Translated".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "translate", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str, input: serde_json::Value) -> WorkflowCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.workflow.video_dir = dir.to_string();
        ctx.pipeline = "dub".to_string();
        ctx.asr_language = Some(crate::r#const::lang::Language::from("zh"));
        ctx
    }

    #[test]
    fn parse_json_reply_finds_brace_block() {
        let v = parse_json_reply("前言 blah {\"dst\": [\"a\",\"b\"]} 后缀").unwrap();
        let arr = v.get("dst").and_then(|d| d.as_array()).unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn chinese_ratio_detects_cjk() {
        assert!(chinese_ratio("你好world") > 0.0);
        assert_eq!(chinese_ratio("hello"), 0.0);
        assert_eq!(chinese_ratio(""), 0.0);
    }

    #[test]
    fn resolve_target_lang_auto() {
        // 源 zh -> en
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let (src, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(src, "zh");
        assert_eq!(dst, "en");

        // 源 en -> zh
        let ctx2 = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let c2 = read_ctx_from_value(json!({
            "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {}, "asr_language": "en"
        }))
        .unwrap();
        let (src2, dst2) = resolve_language(&c2).unwrap();
        assert_eq!(src2, "en");
        assert_eq!(dst2, "zh");
        let _ = ctx2;
    }

    /// 无 ASR (纯 OCR / subtitle 路径) 时源语言回落到 input.workflow.sourceLang,
    /// 否则日语任务会被当成 zh -> 目标语言错误推断为 en。
    #[test]
    fn resolve_src_lang_falls_back_to_workflow_source_lang() {
        let ctx = read_ctx_from_value(json!({
            "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {"workflow": {"sourceLang": "ja"}}
        }))
        .unwrap();
        let (src, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(src, "ja");
        assert_eq!(dst, "zh");
    }

    #[test]
    fn missing_subtitle_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_translate_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = step_translate(&ctx);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("字幕文件不存在"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_roundtrip_restores_completed_batch() {
        let dir = std::env::temp_dir()
            .join(format!("ld_partial_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(format!("{dir}/translate")).unwrap();

        let path = std::path::Path::new(&dir).join("translate/translation.zh.partial.json");
        // 51 句: batch 0 (句0..49) 全部完成; batch 1 仅句50, 缺失
        let n = 51usize;
        let texts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        let srt: Vec<Value> = (0..n)
            .map(|i| json!({"start_ms": i * 100, "end_ms": i * 100 + 50}))
            .collect();
        let mut dsts: Vec<Option<String>> = (0..n).map(|i| Some(format!("D{i}"))).collect();
        dsts[50] = None; // batch 1 那句缺失
        let mut completed = std::collections::HashSet::new();
        completed.insert(0usize); // 仅 batch 0 完成

        partial::write_partial(&path, &texts, &srt, &dsts, &completed, "en", "zh").unwrap();

        let (restored, segs) = partial::read_partial(&path);
        assert!(restored.contains(&0));
        assert_eq!(segs.len(), n);
        // batch 0 内全成功 → 不 missing
        assert!(!segs[0].missing && segs[0].dst == "D0");
        assert!(!segs[49].missing && segs[49].dst == "D49");
        // batch 1 未 completed 且句50 缺失 → missing=true, dst 空
        assert!(segs[50].missing && segs[50].dst.is_empty());

        // 模拟续跑回填: 仅 completed batch 的句恢复, 缺失句仍为 None
        let mut dsts2: Vec<Option<String>> = vec![None; n];
        let mut by_batch: std::collections::HashMap<usize, Vec<&out::TranslatePartialSegment>> =
            std::collections::HashMap::new();
        for ps in segs.iter() {
            by_batch.entry(ps.batch_index).or_default().push(ps);
        }
        for (&bi, segs_b) in by_batch.iter() {
            if !restored.contains(&bi) {
                continue;
            }
            let start = bi * 50;
            for (k, ps) in segs_b.iter().enumerate() {
                let gi = start + k;
                if gi < dsts2.len() && !ps.missing {
                    dsts2[gi] = Some(ps.dst.clone());
                }
            }
        }
        assert_eq!(dsts2[0], Some("D0".to_string()));
        assert_eq!(dsts2[49], Some("D49".to_string()));
        assert_eq!(dsts2[50], None); // 缺失句仍未翻

        let _ = std::fs::remove_dir_all(&dir);
    }
}
