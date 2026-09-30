//! translate 阶段 (镜像 TS `packages/core/steps/05_translate/translate.ts`)。
//!
//! 从权威字幕文件拿时间轴 + 原文, 调 LLM 批量翻译成目标语言, 写出
//! `translate/{lang}.json`。翻译是 dub/subtitle pipeline 共享的 step。
//!
//! 实现要点 (与 TS 对齐):
//! - 目标语言 input > auto 推断 (resolve_language), 由 TranslateOutput::write 落定
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

use crate::steps::translate::out::{TranslateResult, TranslateResultMeta, TranslateSegment};
use crate::steps::translate::prompts::{build_preprocess_prompt, build_translate_system, MetaView};
use crate::steps::utils::{
    lang_name, now_iso, set_step_anyhow, translation_partial_path, StepPatch, StepStatus,
};
use crate::utils::file_ops::write_json_pretty;
use config_rs::env::openai_api_key;

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
///
/// 单参 [`args::TranslateInput`]：字幕来源 + 任务目录 + 本次参数 + 本次语言决策。
pub fn step_translate(input: &args::TranslateInput) -> anyhow::Result<()> {
    tracing::info!(target: "translate", "start");

    let args = &input.args;
    let src_lang = input.src_lang.clone();
    let target_lang = input.target_lang.clone().ok_or_else(|| {
        anyhow::anyhow!("TranslateInput.target_lang 缺失（resolve_language 失败？）")
    })?;
    let video_dir = input.video_dir.clone();

    // **本文件是全仓唯一登记 target_language 的地方**（见 `TranslateOutput::write`
    // 的说明）。放在最前：后续所有产物（translation / split_audio / mix_audio /
    // mix_video）都按这个语言命名，落盘后才能保证「下游读到的」与「translate
    // 实际写的」是同一个语言。
    out::TranslateOutput::write(&video_dir, &target_lang)?;

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
    let srt_segments: Vec<Value> = serde_json::from_value(serde_json::to_value(&srt_segs)?)?;
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

    // 全部完成: 组装正式结果, 写 {lang}.json, 删除 partial
    let segments: Vec<TranslateSegment> = srt_segments
        .iter()
        .enumerate()
        .map(|(idx, u)| TranslateSegment {
            idx: (idx + 1) as u32, // 1-based，与 SplitAudioTiming.seg_idx 对齐
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
    write_json_pretty(out.path(), &result)?;

    // 正式结果已落盘, 删掉增量 partial (避免残留误导; 直通模式无 partial,
    // 但清掉历史遗留也无害)
    let _ = std::fs::remove_file(translation_partial_path(&video_dir, &target_lang));

    // 目标语言事实落盘（本步骤是唯一写者，幂等）
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
    use crate::steps::utils::resolve_language;
    use serde_json::json;

    /// 铺任务目录：`state.json` + `input.json`（+ 可选的 asr 事实），返回 input。
    ///
    /// 源语言的 ASR 实测值现在存在 `<asr>/out.json` 里，所以夹具要真写一份——
    /// 这也是「事实」与「配置」分开之后，测试必须显式选择的一个维度。
    fn at(dir: &str, asr_lang: Option<&str>, input: serde_json::Value) -> crate::input::Input {
        let input = crate::state::write_fixture(
            dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": input,
                "output_mode": "dub"
            }),
        );
        if let Some(lang) = asr_lang {
            crate::steps::asr::out::AsrOutput::write(dir, lang).unwrap();
        }
        input
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
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_auto_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);

        // 源 zh -> en
        let input = at(&dir, Some("zh"), json!({}));
        let (src, dst) = resolve_language(&dir, &input).unwrap();
        assert_eq!(src, "zh");
        assert_eq!(dst, "en");

        // 源 en -> zh
        let input2 = at(&dir, Some("en"), json!({}));
        let (src2, dst2) = resolve_language(&dir, &input2).unwrap();
        assert_eq!(src2, "en");
        assert_eq!(dst2, "zh");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 无 ASR (纯 OCR / subtitle 路径) 时源语言回落到 input.workflow.sourceLang,
    /// 否则日语任务会被当成 zh -> 目标语言错误推断为 en。
    #[test]
    fn resolve_src_lang_falls_back_to_workflow_source_lang() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_ja_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let input = at(&dir, None, json!({"workflow": {"sourceLang": "ja"}}));
        let (src, dst) = resolve_language(&dir, &input).unwrap();
        assert_eq!(src, "ja");
        assert_eq!(dst, "zh");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_subtitle_errors() {
        let dir = std::env::temp_dir()
            .join(format!("ld_translate_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input = at(&dir, Some("zh"), json!({}));
        let res = step_translate(&args::TranslateInput::new(&dir, &input).unwrap());
        let err = res.expect_err("缺 srt.json 必须报错");
        let msg = err.to_string();
        assert!(
            msg.contains("不存在") && msg.contains("asr_fix"),
            "错误应指向缺失的 srt.json 与产出方, 实际: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `subtitleSource` 未知 → `Input` 反序列化阶段 fail-fast（不在契约构造里静默回退）。
    ///
    /// `ctx.input` 已是结构化 [`Input`](crate::input::Input)，未知来源不可能走到
    /// `SubtitleSource::from_input`（它会回退默认 `Asr`）。fail-fast 已上移到 `Input`
    /// 解析，故此处直接断言 `Input` 反序列化拒绝非法值——与 `OutputMode`（读
    /// `ctx.state.output_mode` 事实、未知值报错）的取向一致。
    #[test]
    fn unknown_subtitle_source_rejected_at_input_deserialization() {
        let err = serde_json::from_value::<crate::input::Input>(
            json!({"workflow": {"subtitleSource": "wat"}}),
        )
        .expect_err("未知 subtitleSource 必须在 Input 反序列化阶段报错");
        let msg = err.to_string();
        assert!(
            msg.contains("subtitleSource") || msg.contains("wat"),
            "实际: {msg}"
        );
    }

    /// `subtitleSource` → 对应的 srt.json 路径（provenance 在类型里）。
    #[test]
    fn new_resolves_srt_path_per_source() {
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
            // 先落 srt.json（step_translate 对缺失文件 fail-fast）。
            std::fs::create_dir_all(std::path::Path::new(&dir).join(producer_dir)).unwrap();
            crate::steps::utils::write_srt_json(
                &std::path::Path::new(&dir)
                    .join(producer_dir)
                    .join("srt.json"),
                &[subtitle::SubtitleSegment {
                    text: "t".into(),
                    start_ms: 0,
                    end_ms: 1,
                }],
            )
            .unwrap();

            let input = at(&dir, None, json!({"workflow": {"subtitleSource": source}}));
            let ti =
                args::TranslateInput::new(&dir, &input).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(
                ti.srt_path().to_string_lossy().ends_with(expected),
                "{source} 应解析到 {expected}, 实际 {:?}",
                ti.srt_path()
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

        let path = std::path::Path::new(&dir).join("translate/zh.partial.json");
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

    /// **锁定路径形状**：`TranslateOutput::for_lang` 产出 `translate/{lang}.json`。
    ///
    /// 为什么需要：路径拼装收口到 `LangPath::new` 之后，形状就只由那一处决定。
    /// 若有人「顺手」改了目录名或文件名模板，**所有消费点会一起漂移**——不再有
    /// 第二处旧写法可以对照。这条测试补上那个对照物（原先是与
    /// `translation_file_path` 对比，后者已删）。
    #[test]
    fn translate_output_matches_canonical_path() {
        let out = out::TranslateOutput::for_lang("/w", "vi");
        // 字面路径，防注释与实现分叉
        assert_eq!(out.path().to_string_lossy(), "/w/translate/vi.json");
        assert_eq!(out.lang(), "vi");
        // Display 即路径，错误信息里 {out} 的语义
        assert_eq!(out.to_string(), "/w/translate/vi.json");
    }

    /// `TranslateSegment.idx` 是 **1-based**，且必须与数组位置一致。
    ///
    /// 为什么值这个测试：`idx` 此前是隐式的（各处 `enumerate()` 现推），写进
    /// 产物后就成了一份**可能过期的副本**——若序列化时算错（如写成 0-based），
    /// `split_audio` 会原样搬走 `seg_idx`，错误静默传到 timings/字幕。
    ///
    /// 1-based 的选择也是对 `SplitAudioTiming.seg_idx` 的承诺（后者一直是
    /// `i + 1`）：两者的段号必须能直接对照，换算一次就多一个出错点。
    #[test]
    fn translate_segment_idx_is_one_based_and_dense() {
        let segs: Vec<TranslateSegment> = (0..3)
            .map(|i| TranslateSegment {
                idx: (i + 1) as u32,
                text: format!("t{i}"),
                ..Default::default()
            })
            .collect();

        // 首个是 1 而非 0
        assert_eq!(segs[0].idx, 1, "应为 1-based");
        // 与位置严格对应，无空洞、无重复
        for (i, s) in segs.iter().enumerate() {
            assert_eq!(s.idx as usize, i + 1, "idx 应与位置一致");
        }
    }

    /// `TranslateSegment` 缺 `idx` → 反序列化失败（**刻意不兼容旧产物**）。
    ///
    /// 若给它 `serde(default)`，「字段缺失」与「真的是第 0 段」就分不开，
    /// 而这个字段的全部价值就是可被信任。失败是刻意的：提示重跑 translate，
    /// 而不是拿一个 0 当下标一路传下去。
    #[test]
    fn translate_segment_requires_idx() {
        let missing = r#"{"segments":[{"text":"a","dst":"b","start_ms":0,"end_ms":1}],
                          "meta":{"src_lang":"zh","target_lang":"vi"}}"#;
        let err =
            serde_json::from_str::<TranslateResult>(missing).expect_err("缺 idx 应反序列化失败");
        assert!(
            err.to_string().contains("idx"),
            "错误应点明缺的是 idx, 实际: {err}"
        );

        // 带 idx 才通过
        let ok = r#"{"segments":[{"idx":1,"text":"a","dst":"b","start_ms":0,"end_ms":1}],
                     "meta":{"src_lang":"zh","target_lang":"vi"}}"#;
        let r: TranslateResult = serde_json::from_str(ok).expect("带 idx 应可解析");
        assert_eq!(r.segments[0].idx, 1);
    }

    /// `TranslateResult::read` 走**带类型**反序列化：坏数据在这里就失败。
    ///
    /// 这是改用带类型读取的**全部理由**：此前返 `serde_json::Value`，消费者用
    /// `seg.get("start_ms").and_then(..).unwrap_or(0)` 取值，字段类型不对或缺失
    /// 都会静默变成 0 一路传下去——字幕时序全错但不报错。
    ///
    /// 现在解析集中在一处，且会 fail-fast。
    #[test]
    fn translate_result_read_is_typed_and_fails_fast() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_read_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = crate::steps::utils::paths::translate::LangPath::new(&dir, "vi");
        // LangPath::new 不建目录（它只管路径），这里补上
        std::fs::create_dir_all(p.as_path().parent().unwrap()).unwrap();

        // 缺 idx → 失败（不是读成 0）
        std::fs::write(
            p.as_path(),
            r#"{"segments":[{"text":"a","dst":"b","start_ms":0,"end_ms":1}],
                "meta":{"src_lang":"zh","target_lang":"vi"}}"#,
        )
        .unwrap();
        let err = TranslateResult::read(&p).expect_err("缺 idx 应失败");
        assert!(err.to_string().contains("解析翻译产物"), "实际: {err}");

        // start_ms 是字符串 → 失败（不是静默变 0）
        std::fs::write(
            p.as_path(),
            r#"{"segments":[{"idx":1,"text":"a","dst":"b","start_ms":"x","end_ms":1}],
                "meta":{"src_lang":"zh","target_lang":"vi"}}"#,
        )
        .unwrap();
        TranslateResult::read(&p).expect_err("start_ms 类型错误应失败");

        // 合法 → 读成带类型的值（字段是真的，不是 unwrap_or 兜出来的）
        std::fs::write(
            p.as_path(),
            r#"{"segments":[{"idx":1,"text":"a","dst":"b","start_ms":10,"end_ms":20}],
                "meta":{"src_lang":"zh","target_lang":"vi"}}"#,
        )
        .unwrap();
        let r = TranslateResult::read(&p).expect("合法应可读");
        assert_eq!(r.segments[0].idx, 1);
        assert_eq!(r.segments[0].start_ms, 10);
        assert_eq!(r.meta.src_lang, "zh");

        // 文件不存在 → 失败，且错误带上路径
        let _ = std::fs::remove_file(p.as_path());
        let err = TranslateResult::read(&p).expect_err("文件缺失应失败");
        assert!(
            err.to_string().contains("vi.json"),
            "错误应含路径, 实际: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`out::TranslateOutput`] 只认 `<translate>/out.json`：translate 没跑过 → 报错
    /// （不臆测语言）。
    ///
    /// 这一条钉的是**单源**承诺：读文件与报错必须指向同一路径。若改回
    /// 「现算 `resolve_language`」，当配置与落定事实不同时，报错会指向一个
    /// 没被尝试读取的文件。
    #[test]
    fn translate_output_requires_persisted_target_language() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_out_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);

        // input 里给了 workflow.targetLang（配置），但 translate 还没落定事实——
        // 正是要区分的两件事。
        let input = at(&dir, None, json!({"workflow": {"targetLang": "vi"}}));
        assert!(
            out::TranslateOutput::read(&dir).unwrap().is_none(),
            "前置条件: translate 还没写过 out.json"
        );
        let err = crate::steps::utils::paths::translate::LangPath::from_video_dir(&dir)
            .expect_err("缺 translate/out.json 必须报错");
        assert!(
            err.to_string().contains("translate"),
            "错误应点名缺失的产物, 实际: {err}"
        );
        let _ = input;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **分叉已被约束消除（本测试守护那个约束）**：`resolve_language()` 的现算值
    /// 与 `<translate>/out.json` 里落定的事实仍可能不同——但这**不再会写出错的
    /// 文件**，因为：
    ///
    /// 1. `resolve_language` 是**纯函数**，不写盘；
    /// 2. 落盘只由 `step_translate` 显式调 `TranslateOutput::write` 一次。
    ///
    /// 约束前的问题形状：`resolve_language` 内部「算出来变了就写回」，于是
    /// `split_audio` 先读产物（读旧值）**再** `resolve_language`（顺带改写）——
    /// 读在写之前，读到的文件名与该步骤认定的语言不是同一个。症状：报「文件不
    /// 存在」，但文件在，只是名字不同。
    ///
    /// 本测试断言**两个来源仍可不同**（调用者手里的快照可能陈旧），但不再意味着
    /// 「会污染落定事实」——下半部分验证读盘不变。
    #[test]
    fn resolve_language_is_pure_and_does_not_persist() {
        let dir = std::env::temp_dir()
            .join(format!("ld_tr_diverge_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);

        // translate 落定的事实是 zh；input 里要求 vi —— resolve_language 会算出 vi。
        let input = at(&dir, None, json!({"workflow": {"targetLang": "vi"}}));
        out::TranslateOutput::write(&dir, "zh").unwrap();

        let ti = args::TranslateInput::new(&dir, &input).expect("构造不该失败");

        // ① 两个来源确实可以不同（落定事实 vs 本次推断）
        let (_, resolved) = resolve_language(&dir, &input).unwrap();
        assert_eq!(
            crate::steps::utils::paths::translate::LangPath::from_video_dir(&dir)
                .unwrap()
                .lang(),
            "zh"
        );
        assert_eq!(resolved, "vi");
        let _ = ti;

        // ② **关键**：调 resolve_language 之后，落定的事实必须**没变**。
        //    这是「纯函数」的实质保障——若有人把写回塞回 resolve_language，
        //    这条会红。
        assert_eq!(
            out::TranslateOutput::read(&dir)
                .unwrap()
                .map(|o| o.lang().to_string())
                .as_deref(),
            Some("zh"),
            "resolve_language 不该有副作用；out.json 被改说明写回又回到了它里面"
        );

        // ③ 显式落盘才改（这是 translate 唯一的写点）
        out::TranslateOutput::write(&dir, &resolved).unwrap();
        assert_eq!(
            out::TranslateOutput::read(&dir)
                .unwrap()
                .map(|o| o.lang().to_string())
                .as_deref(),
            Some("vi")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
