//! asr_fix 阶段 (镜像 TS `packages/core/stages/asr/asr_fix.ts`)。

use anyhow::{Context, anyhow};

use crate::context::TaskCtx;
use crate::stages::asr::fix_args::AsrFixArgs;
use crate::stages::asr::out::AsrResult;
use crate::stages::utils::{
    StagePatch, StageStatus, asr_dir, ensure_dir, now_iso, resolve_language,
    set_stage_anyhow,
};

/// 读取 asr_fix 配置 (缺省用 AsrFixArgs::default)。
fn read_fix_args(ctx: &TaskCtx) -> AsrFixArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("asr_fix"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageAsrFix`)。
pub fn stage_asr_fix(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    let args = read_fix_args(ctx);
    if !args.enabled {
        tracing::info!(target: "asr", "disabled (asr_fix.enabled=false), skipping");
        return Ok(());
    }
    // asr_fix 目录平级于 task_dir (镜像 TS `join(taskDir, "asr_fix")`),
    // 与权威字幕路径 (utils::subtitle_path 的 taskDir/asr_fix) 保持一致。
    let asr_fix_dir = std::path::Path::new(&task_dir).join("asr_fix");
    let asr_file = args.asr_file_path.clone().unwrap_or_else(|| {
        asr_dir(&task_dir)
            .join("asr.json")
            .to_string_lossy()
            .to_string()
    });
    let (src_lang, _) = resolve_language(ctx)?;

    if !std::path::Path::new(&asr_file).exists() {
        return Err(anyhow!(
            "ASR file not found: {asr_file}; run ASR stage first"
        ));
    }

    let raw =
        std::fs::read_to_string(&asr_file).with_context(|| format!("读取 {asr_file} 失败"))?;
    let data: AsrResult =
        serde_json::from_str(&raw).with_context(|| format!("解析 {asr_file} 失败"))?;

    let mut segments = data
        .result
        .segments
        .into_iter()
        .filter(|s| {
            !s.text.is_empty()
                && (data.meta.audio_duration == 0 || s.start_ms < data.meta.audio_duration)
        })
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(anyhow!("ASR result has no segments."));
    }

    let llm_fix = args.llm_fix.llm_fix;
    if llm_fix {
        let source_lang_label = lang_label_for(&src_lang);
        let llm_model = args.llm_fix.llm_model.clone();
        let llm_api_base = args.llm_fix.llm_api_base.clone();
        let domain_hint = args.llm_fix.domain_hint.clone();

        if let Some(h) = &domain_hint {
            tracing::info!(target: "asr", "domainHint: {h}");
        }
        set_stage_anyhow(
            &task_dir,
            "asr_fix",
            StagePatch {
                last_message: Some(format!("LLM fixing {} segments...", segments.len())),
                ..Default::default()
            },
        )?;

        let prompt = segments_to_prompt(&segments);
        tracing::info!(target: "asr", 
            "LLM fixing {} segs (model={llm_model})...",
            segments.len()
        );

        let t0 = std::time::Instant::now();
        let system = build_asr_fix_system_prompt(&source_lang_label, domain_hint.as_deref());
        let opts = llm::ChatOptions {
            model: Some(llm_model.clone()),
            api_base: Some(llm_api_base.clone()),
            system_prompt: system,
            api_key: config_rs::env::openai_api_key(),
            ..Default::default()
        };
        let fixed =
            llm::chat_completions(&prompt, &opts).map_err(|e| anyhow!("LLM 修正失败: {e}"))?;
        let elapsed = t0.elapsed().as_secs_f64();

        if let Some(fixed_texts) = llm::parse_lines(&fixed, segments.len()) {
            for (i, s) in segments.iter_mut().enumerate() {
                if let Some(t) = fixed_texts.get(i) {
                    s.text = t.clone();
                }
            }
            tracing::info!(target: "asr", 
                "LLM fixed {} segs in {elapsed:.1}s",
                segments.len()
            );
        } else {
            tracing::info!(target: "asr", "LLM response parse failed, keeping original text");
        }
    }

    let result_text = segments
        .iter()
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    ensure_dir(&asr_fix_dir)?;
    let srt_file = asr_fix_dir.join("asr_fix.json");
    let out = serde_json::json!({
        "result": { "text": result_text, "segments": segments },
        "meta": {
            "audio_duration": data.meta.audio_duration,
            "llm_fixed": llm_fix,
        }
    });
    let json =
        serde_json::to_string_pretty(&out).map_err(|e| anyhow!("序列化 asr_fix 结果失败: {e}"))?;
    std::fs::write(&srt_file, json).with_context(|| format!("写入 {} 失败", srt_file.display()))?;

    tracing::info!(target: "asr", 
        "[ASR Fix] Written {} segs to asr_fix.json",
        segments.len()
    );

    set_stage_anyhow(
        &task_dir,
        "asr_fix",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some(if llm_fix {
                format!("LLM fixed {} segs", segments.len())
            } else {
                "Fixed".into()
            }),
            ..Default::default()
        },
    )?;

    Ok(())
}

/// 语言码 -> 展示名 (镜像 TS `t(srcLang)`)。
fn lang_label_for(code: &str) -> String {
    llm::lang_label(code).to_string()
}

/// 构造 LLM 修正 prompt (镜像 TS `segmentsToPrompt`)。
fn segments_to_prompt(segments: &[crate::stages::asr::out::AsrSegment]) -> String {
    let full_text = segments
        .iter()
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    let lines = segments
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}: {}", i + 1, s.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "全文上下文（参考用，每句以空格分隔）：\n{full_text}\n\n请修正以下条目（保持行号不变）：\n{lines}"
    )
}

/// 构造 asr 修正系统提示 (镜像 TS `buildAsrFixSystemPrompt`)。
fn build_asr_fix_system_prompt(lang: &str, domain_hint: Option<&str>) -> String {
    let mut prompt = "你是一个 ASR 纠错助手。修正中文转录文本中的错别字。\n\
\n\
输入包含两部分：\n\
1. \"全文上下文\" — 完整对话，帮助理解语境\n\
2. \"请修正以下条目\" — 按行号列出的待修正文本\n\
\n\
规则：\n\
1. 先参考全文上下文理解语境，再逐条修正\n\
2. 保持行号不变\n\
3. 只修改文字错误，不改标点\n\
4. 保持行数完全一致\n\
5. 不要添加解释或额外内容\n\
6. 没有错误的行保持原样\n\
7. 注意：中文 ASR 常见同音/近音字错误，根据上下文判断正确用词"
        .to_string();
    if let Some(h) = domain_hint {
        prompt.push_str(&format!("\n\n领域提示：{h}"));
    }
    let _ = lang;
    prompt
}
