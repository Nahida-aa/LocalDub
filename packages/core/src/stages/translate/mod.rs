//! translate 阶段 (镜像 TS `packages/core/stages/05_translate/translate.ts`)。
//!
//! 从权威字幕文件拿时间轴 + 原文, 调 LLM 批量翻译成目标语言, 写出
//! `translate/translation.{lang}.json`。翻译是 dub/subtitle pipeline 共享的 stage。
//!
//! 实现要点 (与 TS 对齐):
//! - 目标语言 input > auto 推断 (resolve_language), 写回 ctx.target_language
//! - 有 ytdlp_info.json 时先做 preprocess (summary/hotwords/corrections) 注入系统提示
//! - 批量 BATCH_SIZE=50, 每批 3 次重试; 目标非中文时校验译文中文占比 < 0.3
//! - 破折号 `——` 替换为逗号

pub mod args;
pub mod out;
pub mod prompts;

use std::path::Path;

use serde_json::Value;

use crate::context::TaskCtx;
use crate::stages::translate::out::{TranslateResult, TranslateResultMeta, TranslateSegment};
use crate::stages::translate::prompts::{
    MetaView, build_preprocess_prompt, build_translate_system,
};
use crate::stages::utils::{
    StagePatch, StageStatus, lang_name, now_iso, resolve_language, set_stage_anyhow,
    subtitle_file_path, translation_file_path,
};
use config_rs::env::openai_api_key;

/// 从 `ctx.input.stages.translate` 解析配置 (镜像 TS `readInputArgs().stages.translate`)。
fn read_args(ctx: &TaskCtx) -> args::TranslateArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("translate"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
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

/// 批量翻译 (含 3 次重试), 返回与 `batch` 等长的译文 (镜像 TS `translateBatch`)。
fn translate_batch(
    batch: &[String],
    system: &str,
    target_lang: &str,
    args: &args::TranslateArgs,
) -> anyhow::Result<Vec<String>> {
    let numbered: String = batch
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    let mut last_err = String::new();
    for attempt in 0..3 {
        let user_msg = if attempt > 0 {
            format!(
                "{numbered}\n\n（注意：以上回复包含中文！必须全部输出{lang}译文，不得包含任何中文。）",
                numbered = numbered,
                lang = lang_name(target_lang)
            )
        } else {
            numbered.clone()
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
                last_err = e.to_string();
                continue;
            }
        };
        let parsed = match parse_json_reply(&reply) {
            Ok(v) => v,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        let arr = parsed.get("dst").and_then(|d| d.as_array()).ok_or_else(|| {
            last_err = "LLM 回复缺少 dst 数组".to_string();
            anyhow::anyhow!("LLM 回复缺少 dst 数组")
        });
        let arr = match arr {
            Ok(a) => a,
            Err(e) => {
                if attempt < 2 {
                    continue;
                }
                return Err(e);
            }
        };
        let mut results: Vec<String> = Vec::with_capacity(batch.len());
        let mut ok = true;
        for (i, d) in arr.iter().take(batch.len()).enumerate() {
            let dst = d.as_str().unwrap_or("").trim().to_string();
            if target_lang != "zh" && chinese_ratio(&dst) > 0.3 {
                last_err = format!(
                    "第 {} 句仍含中文 (ratio={:.2}, 期望 {})",
                    i + 1,
                    chinese_ratio(&dst),
                    target_lang
                );
                ok = false;
                break;
            }
            if dst.is_empty() {
                last_err = format!("第 {} 句译文为空", i + 1);
                ok = false;
                break;
            }
            results.push(dst);
        }
        if ok && results.len() == batch.len() {
            return Ok(results);
        }
        if attempt == 2 {
            return Err(anyhow::anyhow!(
                "批量翻译 3 次重试后仍失败: {last_err} (期望 {target_lang})"
            ));
        }
    }
    Err(anyhow::anyhow!(
        "批量翻译失败 (3 次): {last_err} (期望 {target_lang})"
    ))
}

/// 入口 (镜像 TS `stageTranslate`)。
pub fn stage_translate(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    tracing::info!(target: "translate", "translate: start");

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
    let ytdlp_path = Path::new(&task_dir)
        .join("download")
        .join("ytdlp_info.json");
    let has_meta = ytdlp_path.exists();
    let mut meta = MetaView::default();
    if has_meta {
        if let Ok(raw) = std::fs::read_to_string(&ytdlp_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                meta = MetaView {
                    title: v
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(500)
                        .collect(),
                    uploader: v
                        .get("uploader")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(200)
                        .collect(),
                    description: v
                        .get("description")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(500)
                        .collect::<String>()
                        .replace("(none)", ""),
                };
                if meta.description.is_empty() {
                    meta.description = "(none)".to_string();
                }
            }
        }
    }

    // preprocess (仅在有元信息时)
    let mut summary = String::new();
    let mut hotwords_str = "(none)".to_string();
    let mut corrections_str = "(none)".to_string();
    if has_meta {
        let preprocess = build_preprocess_prompt(&dst_lang_name, &src_lang_name, &meta, &full_text);
        match llm::chat_completions(
            &preprocess,
            &llm::ChatOptions {
                model: Some(args.model.clone()),
                api_base: Some(args.api_base.clone()),
                system_prompt: "You output strict JSON only.".to_string(),
                api_key: openai_api_key(),
                max_tokens: Some(2048),
                temperature: Some(0.2),
            },
        ) {
            Ok(raw) => {
                if let Ok(v) = parse_json_reply(&raw) {
                    summary = v
                        .get("summary")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let hotwords: Vec<String> = v
                        .get("hotwords")
                        .and_then(|h| h.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|h| {
                                    let s = h.get("src").and_then(|x| x.as_str())?;
                                    let d = h.get("dst").and_then(|x| x.as_str())?;
                                    Some(format!("{s} -> {d}"))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let corrections: Vec<String> = v
                        .get("corrections")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|c| {
                                    let w = c.get("wrong").and_then(|x| x.as_str())?;
                                    let r = c.get("correct").and_then(|x| x.as_str())?;
                                    Some(format!("{w} -> {r}"))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if !hotwords.is_empty() {
                        hotwords_str = hotwords.join("\n");
                    }
                    if !corrections.is_empty() {
                        corrections_str = corrections.join("\n");
                    }
                }
            }
            Err(e) => tracing::warn!(target: "translate", "[Translate] Preprocess failed: {e}"),
        }
    }

    let system = build_translate_system(
        &dst_lang_name,
        &src_lang_name,
        &meta,
        &summary,
        &hotwords_str,
        &corrections_str,
    );

    const BATCH_SIZE: usize = 50;
    let mut dsts: Vec<String> = Vec::with_capacity(texts.len());
    for (i, chunk) in texts.chunks(BATCH_SIZE).enumerate() {
        let batch: Vec<String> = chunk.to_vec();
        let results = translate_batch(&batch, &system, &target_lang, &args)?;
        dsts.extend(results);
        set_stage_anyhow(
            &task_dir,
            "translate",
            StagePatch {
                last_message: Some(format!(
                    "Translating {}/{}...",
                    (i + 1) * BATCH_SIZE,
                    texts.len()
                )),
                ..Default::default()
            },
        )
        .ok();
    }

    let segments: Vec<TranslateSegment> = srt_segments
        .iter()
        .enumerate()
        .map(|(idx, u)| TranslateSegment {
            text: texts[idx].clone(),
            dst: dsts
                .get(idx)
                .map(|d| d.replace("——", "，"))
                .unwrap_or_default(),
            src_lang: Some(src_lang.clone()),
            dst_lang: Some(target_lang.clone()),
            start_ms: u.get("start_ms").and_then(|v| v.as_u64()).unwrap_or(0),
            end_ms: u.get("end_ms").and_then(|v| v.as_u64()).unwrap_or(0),
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

    let out_file = translation_file_path(&task_dir, &target_lang);
    if let Some(parent) = out_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("序列化翻译结果失败: {e}"))?;
    std::fs::write(&out_file, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", out_file.display(), e))?;

    // 确保 ctx.target_language 在内存之外也落盘 (resolve_language 已写回文件, 这里仅保险)
    set_stage_anyhow(
        &task_dir,
        "translate",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Translated".to_string()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "translate", "translate: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = dir.to_string();
        ctx.pipeline = "dub".to_string();
        ctx.asr_language = Some("zh".to_string());
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
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
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
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        let c2 = read_ctx_from_value(json!({
            "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {}, "asr_language": "en"
        }))
        .unwrap();
        let (src2, dst2) = resolve_language(&c2).unwrap();
        assert_eq!(src2, "en");
        assert_eq!(dst2, "zh");
        let _ = ctx2;
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
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_translate(&ctx);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("字幕文件不存在"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
