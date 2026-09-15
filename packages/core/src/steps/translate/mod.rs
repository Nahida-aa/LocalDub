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
pub mod batch;
pub mod out;
pub mod partial;
pub mod prompts;

use std::path::Path;

use serde_json::Value;

use crate::context::WorkflowCtx;
use crate::steps::translate::out::{TranslateResult, TranslateResultMeta, TranslateSegment};
use crate::steps::translate::prompts::{build_preprocess_prompt, build_translate_system, MetaView};
use crate::steps::utils::{
    lang_name,     now_iso, resolve_language, set_step_anyhow, translation_partial_path, StepPatch, StepStatus,
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

/// 读视频元信息 → [`MetaView`]。
///
/// `None`（ytdlp_info.json 不存在）或内容非法都退化为 `MetaView::default()`——
/// 元信息只用于给 LLM 加上下文, 缺了照样能翻, 不该让 step 失败。
/// 字段做了长度截断 (避免把整段视频简介塞进提示词)。
fn load_video_meta(path: Option<&Path>) -> MetaView {
    let Some(path) = path else {
        return MetaView::default();
    };
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
///
/// **被 `batch` 与 `run_preprocess` 共用**, 所以留在 mod 层 (避免 batch 反向
/// 依赖调用方)。
pub fn parse_json_reply(raw: &str) -> anyhow::Result<Value> {
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

/// 入口 (镜像 TS `stepTranslate`)。
pub fn step_translate(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let video_dir = ctx.workflow.video_dir.clone();
    tracing::info!(target: "translate", "start");

    let input = args::TranslateInput::from_ctx(ctx)?;
    let args = read_args(ctx);
    let (src_lang, target_lang) = resolve_language(ctx)?;
    let src_lang_name = lang_name(&src_lang);
    let dst_lang_name = lang_name(&target_lang);

    // srt.json 的存在性语义按 `translate.enabled` 分：
    // - enabled：缺失/空 → Err（要求翻译却没有可翻译的内容 = 配置或上游问题）
    // - disabled：缺失 → 空 segments 直通（上游被禁用, 本来就没有字幕）；
    //   直通把原文写进 translation.json，下游步骤从此只认这一种字幕来源。
    let srt_exists = input.srt_path().exists();
    let srt_segs: Vec<subtitle::SubtitleSegment> = if srt_exists {
        crate::steps::utils::read_srt_json(input.srt_path())?
    } else if args.enabled {
        return Err(anyhow::anyhow!(
            "规范化字幕 {} 不存在；请先运行 {}（或从该步骤 continue 以重新生成）",
            input.srt_path().display(),
            input.producer()
        ));
    } else {
        Vec::new()
    };
    if args.enabled && srt_segs.is_empty() {
        return Err(anyhow::anyhow!(
            "{} 无 segments",
            input.srt_path().display()
        ));
    }
    let srt_segments: Vec<Value> =
        serde_json::from_value(serde_json::to_value(&srt_segs)?)?;
    let texts: Vec<String> = srt_segs.iter().map(|s| s.text.trim().to_string()).collect();

    let dsts: Vec<Option<String>> = if !args.enabled {
        // ── 直通：原文即译文 ──
        // 不调 LLM、不写 partial。translation.json 的 dst 填原文，
        // 下游 (split_audio/tts/mix_video) 与翻译开启时走完全相同的路。
        tracing::info!(target: "translate",
            "translate.enabled=false → 直通: {} 段原文作为译文", texts.len());
        texts.iter().cloned().map(Some).collect()
    } else {
        let full_text = texts.join(" ");

        // preprocess (仅在有元信息时): 让 LLM 先读一遍全文, 产出摘要/热词/纠错,
        // 注入后续批量翻译的系统提示。失败不阻断——退化成"无预处理"直接翻。
        let ytdlp_path = Path::new(&video_dir)
            .join("download")
            .join("ytdlp_info.json");
        let meta = load_video_meta(Some(&ytdlp_path));
        let pre = if ytdlp_path.exists() {
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
            let mut by_batch:
                std::collections::HashMap<usize, Vec<&out::TranslatePartialSegment>> =
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
            match batch::translate_batch(&batch, &system, &target_lang, &args) {
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
        dsts
    };

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

    let out = out::TranslateOutput::for_lang(&video_dir, &target_lang);
    if let Some(parent) = out.path().parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("序列化翻译结果失败: {e}"))?;
    std::fs::write(out.path(), json)
        .map_err(|e| anyhow::anyhow!("写入 {out} 失败: {e}"))?;

    // 正式结果已落盘, 删掉增量 partial (避免残留误导; 直通模式无 partial,
    // 但清掉历史遗留也无害)
    let _ = std::fs::remove_file(translation_partial_path(&video_dir, &target_lang));

    // 确保 ctx.target_language 在内存之外也落盘 (resolve_language 已写回文件, 这里仅保险)
    set_step_anyhow(
        &video_dir,
        "translate",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some(if args.enabled {
                "Translated".to_string()
            } else {
                "Translated (passthrough, 原文作为译文)".to_string()
            }),
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
        ctx.output_mode = "dub".to_string();
        ctx.asr_language = Some(crate::r#const::lang::Language::from("zh"));
        ctx
    }

    /// `parse_json_reply` 由 batch 与 run_preprocess 共用, 留在 mod 层测。
    #[test]
    fn parse_json_reply_finds_brace_block() {
        let v = parse_json_reply("前言 blah {\"dst\": [\"a\",\"b\"]} 后缀").unwrap();
        let arr = v.get("dst").and_then(|d| d.as_array()).unwrap();
        assert_eq!(arr.len(), 2);
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
        let err = res.expect_err("缺 srt.json 必须报错");
        let msg = err.to_string();
        assert!(
            msg.contains("不存在") && msg.contains("asr_fix"),
            "错误应指向缺失的 srt.json 与产出方, 实际: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `subtitleSource` 未知 → from_ctx 显式报错（fail-fast，不静默回退）。
    #[test]
    fn unknown_subtitle_source_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_translate_src_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"subtitleSource": "wat"}}
            }),
        );
        let err = args::TranslateInput::from_ctx(&ctx).expect_err("未知来源必须报错");
        assert!(
            err.to_string().contains("未知的 subtitleSource"),
            "实际: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `subtitleSource` → 对应的 srt.json 路径（provenance 在类型里）。
    #[test]
    fn from_ctx_resolves_srt_path_per_source() {
        let dir = std::env::temp_dir()
            .join(format!("ld_translate_p_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        for (source, producer_dir, expected) in [
            ("asr", "asr_fix", "asr_fix/srt.json"),
            ("sf_ocr", "sf_ocr_fix", "sf_ocr_fix/srt.json"),
            ("asr_ocr", "asr_ocr_fix", "asr_ocr_fix/srt.json"),
        ] {
            // 先落 srt.json（from_ctx 对缺失文件 fail-fast）。
            std::fs::create_dir_all(std::path::Path::new(&dir).join(producer_dir)).unwrap();
            crate::steps::utils::write_srt_json(
                &std::path::Path::new(&dir).join(producer_dir).join("srt.json"),
                &[subtitle::SubtitleSegment {
                    text: "t".into(),
                    start_ms: 0,
                    end_ms: 1,
                }],
            )
            .unwrap();

            let ctx = ctx_at(
                &dir,
                json!({
                    "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                             "status":"running","created_at":"2024-01-01T00:00:00Z"},
                    "input": {"workflow": {"subtitleSource": source}}
                }),
            );
            let input = args::TranslateInput::from_ctx(&ctx)
                .unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(
                input.srt_path().to_string_lossy().ends_with(expected),
                "{source} 应解析到 {expected}, 实际 {:?}",
                input.srt_path()
            );
        }
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

    /// **锁定唯一构造入口**：`TranslateOutput::for_lang` 必须与
    /// `translation_file_path` 产出**逐字相同**的路径。
    ///
    /// 为什么需要：改动把 5 处拼接收拢到 `for_lang` 之后，路径形状就只由这一处决定。
    /// 若有人「顺手」在 `for_lang` 里改了目录名或文件名模板，**所有消费点会一起
    /// 漂移**——不再有任何一处保留旧写法可以对照。这条测试补上那个对照物。
    #[test]
    fn translate_output_matches_canonical_path() {
        let out = out::TranslateOutput::for_lang("/w", "vi");
        assert_eq!(
            out.path(),
            &crate::steps::utils::translation_file_path("/w", "vi"),
            "for_lang 必须与 translation_file_path 一致"
        );
        // 真实模板，防止注释与实现再次分叉（paths.rs 曾写成 translate/{lang}.json）
        assert_eq!(out.path().to_string_lossy(), "/w/translate/translation.vi.json");
        assert_eq!(out.lang(), "vi");
        // Display 即路径，错误信息里 {out} 的语义
        assert_eq!(out.to_string(), "/w/translate/translation.vi.json");
    }

    /// `from_ctx` 只认 `ctx.target_language`：未设置 → 报错（不臆测语言）。
    ///
    /// 这一条钉的是**单源**承诺：读文件与报错必须指向同一路径。若改回
    /// 「现算 `resolve_language`」，当 ctx.json 快照与现算结果不同时，
    /// 报错会指向一个没被尝试读取的文件。
    #[test]
    fn translate_output_from_ctx_requires_target_language() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_out_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // ctx_at 的 target_language 来自 ctx.target_language 字段；input 里只给
        // workflow.targetLang（配置），不足以让 from_ctx 成功——正是要区分的两件事。
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"targetLang": "vi"}}
            }),
        );
        assert!(
            ctx.target_language.is_none(),
            "前置条件: ctx.target_language 应为 None"
        );
        let err = out::TranslateOutput::from_ctx(&ctx).expect_err("缺 target_language 必须报错");
        assert!(
            err.to_string().contains("target_language"),
            "错误应点名缺失的字段, 实际: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
