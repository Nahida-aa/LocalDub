//! 用 workflow-core 引擎 (aa-workflow) 驱动 pipeline (Phase 1: 串行等价)。
//!
//! 目标: 与 [`crate::workflows::pipeline::run_pipeline`] 对同样的 step 序列、同样的
//! ctx.json 副作用达成一致, 但"成败真相"从 ctx.json 转移为
//! `<workflow_dir>/workflow-engine/events.jsonl` (见 [`engine_store::FsRunStore`])。
//!
//! 建立方式: 每个活跃 step (过滤掉无 handler 的) 建成一个 step, 依赖前一个已注册 step
//! (显式链式 DAG), `max_concurrency = 1` 强制串行。step 闭包内部复用
//! [`crate::workflows::pipeline::run_step`], 前后照旧写 ctx.json (UI 投影)。
//!
//! 与 run_pipeline 的差异点 (引擎天然语义):
//! - **resume**: 同 run_id 重跑时跳过事件日志中已 `success` 的 step。
//! - **continue_from**: 把目标 step 起的后缀 step 重置为 pending 再续跑
//!   (镜像 `continue_pipeline` 在 ctx.json 上的重置)。
//! - **target_step**: 命中即停, 标记成功 (镜像 run_pipeline 的 targetStep)。
//! - 阶段错误 → 事件日志记 StepFailed, engine 停, workflow 置 failed。

use std::sync::Arc;

use workflow_core::{run_workflow, RunOptions, RunStatus, StepSpec, StepContext};

use crate::context::read_ctx;
use crate::steps::get_steps;
use crate::steps::utils::{
    now_iso, set_step_anyhow, set_workflow_anyhow, video_id, StepPatch, StepStatus, WorkflowPatch,
};
use crate::workflows::pipeline::{has_handler, run_step};

use super::engine_store::FsRunStore;

/// 引擎调用参数 (由调用方注入; 未显式给出的回退到 ctx.input.workflow 里的字段)。
#[derive(Default, Debug, Clone)]
pub struct EngineOptions {
    /// 从该 step 起重置为 pending 续跑 (镜像 continue_pipeline 的 continueFrom)。
    pub continue_from: Option<String>,
    /// 命中该 step 即停 (镜像 run_pipeline 的 targetStep)。
    pub target_step: Option<String>,
}

impl EngineOptions {
    /// 从 `ctx.input` 填充: `workflow.continueFrom` / `workflow.targetStep`
    /// (TS 续跑路径放这); `run_pipeline` 的内联约定 `targetStep` 顶层字段也兜底读取。
    pub fn from_ctx_input(input: &serde_json::Value) -> Self {
        let wf = input.get("workflow");
        let get = |k: &str| {
            wf.and_then(|v| v.get(k))
                .and_then(|v| v.as_str())
                .map(String::from)
        };
        let target_step = get("targetStep").or_else(|| {
            input
                .get("targetStep")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
        Self {
            continue_from: get("continueFrom"),
            target_step,
        }
    }
}

/// step 成功后的产物路径清单, 作为 step 返回值写入事件日志
/// (`StepFinished.result`, 对齐 TanStack STEP_FINISHED 的 value 语义)。
/// 返回 `None` 表示无产物 / 无法读取 (不使 step 失败)。
fn step_artifacts(workflow_dir: &str, step: &str) -> Option<serde_json::Value> {
    use crate::steps::utils::{
        asr_dir, asr_ocr_dir, asr_ocr_fix_dir, asr_ocr_pre_dir, dubbing_path, final_video_dir,
        gated_vocals_path, mix_audio_timings_path, mixed_vocals_path, resolve_language,
        separate_dir, sf_ocr_dir, sf_ocr_fix_dir, sf_ocr_pre_dir, split_audio_path,
        split_audio_timings_path, subtitle_file_path, tts_filepath, video_id,
    };
    let wf = workflow_dir;
    let p = |pb: std::path::PathBuf| pb.to_string_lossy().into_owned();
    let artifacts: Vec<String> = match step {
        // leaf: 精确产物文件
        "separate" => vec![p(separate_dir(wf))],
        "separate_after" => vec![p(mixed_vocals_path(wf)), p(gated_vocals_path(wf))],
        "asr" => vec![p(asr_dir(wf))],
        "asr_fix" => vec![subtitle_file_path(&crate::context::read_ctx(wf).ok()?)],
        "translate" => {
            let ctx = crate::context::read_ctx(wf).ok()?;
            let (_, lang) = resolve_language(&ctx).ok()?;
            vec![crate::steps::utils::translation_file_path(wf, &lang)
                .to_string_lossy()
                .into_owned()]
        }
        "split_audio" => vec![p(split_audio_path(wf)), p(split_audio_timings_path(wf))],
        "tts" => vec![p(tts_filepath(wf))],
        "mix_audio" => vec![p(dubbing_path(wf)), p(mix_audio_timings_path(wf))],
        "mix_video" => {
            let ctx = crate::context::read_ctx(wf).ok()?;
            let no_translate = ctx
                .input
                .get("steps")
                .and_then(|v| v.get("translate"))
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool())
                == Some(false);
            let sub_src = ctx
                .input
                .get("workflow")
                .and_then(|v| v.get("subtitleSource"))
                .and_then(|v| v.as_str())
                .unwrap_or("asr");
            let dir_name = final_video_dir(&ctx.pipeline, sub_src, no_translate);
            let vid = video_id(wf).unwrap_or_default();
            vec![p(std::path::Path::new(wf)
                .join("mix_video")
                .join(dir_name)
                .join(format!("{vid}.mp4")))]
        }
        // 中间层: 输出目录
        "sf_ocr_pre" => vec![p(sf_ocr_pre_dir(wf))],
        "sf_ocr" => vec![p(sf_ocr_dir(wf))],
        "sf_ocr_fix" => vec![p(sf_ocr_fix_dir(wf))],
        "asr_ocr_pre" => vec![p(asr_ocr_pre_dir(wf))],
        "asr_ocr" => vec![p(asr_ocr_dir(wf))],
        "asr_ocr_fix" => vec![p(asr_ocr_fix_dir(wf))],
        _ => return None,
    };
    Some(serde_json::json!({ "artifacts": artifacts }))
}

/// 用引擎跑 (或续跑) 一条 pipeline, 结束后 workflow.status ∈ {success, failed}。
pub fn run_workflow_engine(workflow_dir: &str, opts: &EngineOptions) -> anyhow::Result<()> {
    let _guard = tracing::info_span!("workflow", workflow_dir = workflow_dir).entered();
    let ctx = read_ctx(workflow_dir).map_err(anyhow::Error::msg)?;
    let steps = get_steps(&ctx);

    let target_step = opts
        .target_step
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).target_step);
    let continue_from = opts
        .continue_from
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).continue_from);

    // target_step 不在序列中则告警忽略 (镜像 run_pipeline）
    if let Some(ts) = &target_step {
        if !steps.iter().any(|s| s == ts) {
            tracing::info!(target: "engine",
                "[WARN] target_step \"{ts}\" 不在 {} pipeline 中, 忽略", ctx.pipeline);
        }
    }

    // continue_from → 先把后缀阶段在 ctx.json 重置为 pending (镜像 continue_pipeline)
    if let Some(cf) = &continue_from {
        let idx = steps
            .iter()
            .position(|s| s == cf)
            .ok_or_else(|| anyhow::anyhow!("Unknown step \"{cf}\""))?;
        for s in &steps[idx..] {
            set_step_anyhow(
                workflow_dir,
                s,
                StepPatch {
                    status: Some(StepStatus::Pending),
                    ..Default::default()
                },
            )?;
        }
        tracing::info!(target: "engine",
            "Resetting from \"{cf}\" ({} step(s))", steps.len() - idx);
    }

    set_workflow_anyhow(
        workflow_dir,
        WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            ..Default::default()
        },
    )?;

    // 构建显式链式 DAG: 每个已注册阶段依赖前一个已注册阶段。
    let run_id =
        video_id(workflow_dir).ok_or_else(|| anyhow::anyhow!("无法从 workflow_dir 取 run_id"))?;
    let mut wf = workflow_core::Workflow::new(ctx.pipeline.clone());
    let mut prev: Option<String> = None;
    for step in &steps {
        if !has_handler(step) {
            tracing::info!(target: "engine", "[WARN] No handler for step {step}, skipping");
            continue;
        }
        let dir = workflow_dir.to_string();
        let name = step.clone();
        let step_name = name.clone();
        let mut spec = StepSpec::new(
            name.clone(),
            Arc::new(move |_ctx: StepContext| {
                tracing::info!(target: "engine", "Running {step_name}");
                set_step_anyhow(
                    &dir,
                    &step_name,
                    StepPatch {
                        status: Some(StepStatus::Running),
                        started_at: Some(now_iso()),
                        last_message: Some(format!("Starting {step_name}...")),
                        ..Default::default()
                    },
                )?;
                set_workflow_anyhow(
                    &dir,
                    WorkflowPatch {
                        status: Some("running".to_string()),
                        current_step: Some(Some(step_name.clone())),
                        ..Default::default()
                    },
                )?;
                match run_step(&step_name, &dir) {
                    Ok(()) => Ok(step_artifacts(&dir, &step_name)),
                    Err(e) => {
                        let msg = e.to_string();
                        tracing::error!(target: "engine", "Step {step_name} failed: {msg}");
                        set_step_anyhow(
                            &dir,
                            &step_name,
                            StepPatch {
                                status: Some(StepStatus::Failed),
                                error_message: Some(msg.clone()),
                                completed_at: Some(now_iso()),
                                ..Default::default()
                            },
                        )?;
                        set_workflow_anyhow(
                            &dir,
                            WorkflowPatch {
                                status: Some("failed".to_string()),
                                error_message: Some(msg.clone()),
                                ..Default::default()
                            },
                        )?;
                        Err(anyhow::anyhow!(msg))
                    }
                }
            }),
        );
        if let Some(p) = &prev {
            spec = spec.needs([p.clone()]);
        }
        prev = Some(name);
        wf = wf.step(spec);
    }

    let store = FsRunStore::new(workflow_dir);
    let mut r_opts = RunOptions::new(ctx.input.clone())
        .run_id(run_id.clone())
        .max_concurrency(1);
    if let Some(cf) = &continue_from {
        r_opts = r_opts.continue_from(cf.clone());
    }
    if let Some(ts) = &target_step {
        r_opts = r_opts.target_step(ts.clone());
    }

    let outcome = run_workflow(&mut wf, &store, &r_opts, None)
        .map_err(|e| anyhow::anyhow!("workflow engine error: {e}"))?;

    match outcome.status {
        RunStatus::Finished => {
            set_workflow_anyhow(
                workflow_dir,
                WorkflowPatch {
                    status: Some("success".to_string()),
                    completed_at: Some(now_iso()),
                    current_step: Some(None),
                    ..Default::default()
                },
            )?;
            tracing::info!(target: "engine", "workflow {run_id} completed via engine");
            Ok(())
        }
        RunStatus::Errored => {
            let msg = outcome
                .error
                .unwrap_or_else(|| "unknown engine error".to_string());
            set_workflow_anyhow(
                workflow_dir,
                WorkflowPatch {
                    status: Some("failed".to_string()),
                    error_message: Some(msg.clone()),
                    ..Default::default()
                },
            )?;
            Err(anyhow::anyhow!(msg))
        }
        other => Err(anyhow::anyhow!("unexpected engine status: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{read_ctx, read_ctx_from_value, write_ctx, StepStatus};
    use serde_json::json;
    use workflow_core::RunStore;

    fn temp_dir(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!("ld_engine_{name}_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 构造写好 ctx.json 的 subtitle 临时 workflow 目录 (只触发 separate / separate_after).
    fn setup_subtitle(dir: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let input = json!({
            "steps": {
                "separate": {"always": false},
                "asr": {"enabled": false},
                "asr_fix": {"enabled": false},
                "translate": {"enabled": false},
                "mix_video": {"enabled": false}
            }
        });
        let mut ctx = read_ctx_from_value(json!({
            "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": input,
            "pipeline": "subtitle"
        }))
        .unwrap();
        ctx.workflow.workflow_dir = dir.to_string();
        ctx.workflow.id = "t".to_string();
        ctx.pipeline = "subtitle".to_string();
        write_ctx(dir, &ctx).unwrap();
    }

    fn status_of(dir: &str, step: &str) -> StepStatus {
        let ctx = read_ctx(dir).unwrap();
        ctx.steps
            .unwrap()
            .iter()
            .find(|s| s.name == step)
            .unwrap()
            .status
    }

    #[test]
    fn engine_serial_matches_run_pipeline() {
        let dir_old = temp_dir("eq_old");
        let dir_new = temp_dir("eq_new");
        setup_subtitle(&dir_old);
        setup_subtitle(&dir_new);

        crate::workflows::pipeline::run_pipeline(&dir_old).expect("run_pipeline 不应失败");
        run_workflow_engine(&dir_new, &EngineOptions::default()).expect("engine 不应失败");

        for dir in [&dir_old, &dir_new] {
            let ctx = read_ctx(dir).unwrap();
            assert_eq!(ctx.workflow.status, "success");
            assert_eq!(ctx.workflow.current_step, None);
            assert_eq!(status_of(dir, "separate"), StepStatus::Success);
            assert_eq!(status_of(dir, "separate_after"), StepStatus::Success);
        }

        let _ = std::fs::remove_dir_all(&dir_old);
        let _ = std::fs::remove_dir_all(&dir_new);
    }

    #[test]
    fn engine_resume_skips_success_nodes() {
        let dir = temp_dir("resume");
        setup_subtitle(&dir);

        run_workflow_engine(&dir, &EngineOptions::default()).unwrap();

        // 事件日志: 每个 step 只跑一次 → 只应有 1 次 StepFinished
        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        let n_finished = |step: &str| {
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::StepFinished { step_id, .. } if step_id == step))
                .count()
        };
        assert_eq!(n_finished("separate"), 1);
        assert_eq!(n_finished("separate_after"), 1);

        // 再次运行 → 全部命中事件日志 success, 无 step 重跑
        run_workflow_engine(&dir, &EngineOptions::default()).unwrap();
        let events = store.get_events("t").unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::StepFinished { step_id, .. } if step_id == "separate"))
                .count(),
            1
        );
        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_target_step_stops() {
        let dir = temp_dir("target");
        setup_subtitle(&dir);

        run_workflow_engine(
            &dir,
            &EngineOptions {
                target_step: Some("separate".to_string()),
                ..EngineOptions::default()
            },
        )
        .unwrap();

        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");
        assert_eq!(status_of(&dir, "separate"), StepStatus::Success);
        // separate_after 未跑 → ctx.json 无对应 step 条目 (或非 success)
        let after = ctx
            .steps
            .as_ref()
            .and_then(|s| s.iter().find(|s| s.name == "separate_after"));
        assert!(after
            .map(|s| s.status != StepStatus::Success)
            .unwrap_or(true));

        // 事件日志里 separate_after 不应有 StepFinished
        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        assert!(
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::StepFinished { .. }))
                .all(|e| e.step_id() != Some("separate_after")),
            "separate_after 不应被引擎执行"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_records_step_artifacts_in_events() {
        let dir = temp_dir("artifacts");
        setup_subtitle(&dir);

        run_workflow_engine(&dir, &EngineOptions::default()).unwrap();

        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        let separate_res = events
            .iter()
            .find_map(|e| match e {
                workflow_core::RunEvent::StepFinished {
                    step_id, result, ..
                } if step_id == "separate" => result.clone(),
                _ => None,
            })
            .expect("separate StepFinished 应有 result");
        let artifacts = separate_res.get("artifacts").and_then(|v| v.as_array());
        let a = artifacts.expect("result.artifacts 应为数组");
        assert_eq!(a.len(), 1);
        assert_eq!(
            a[0].as_str().unwrap(),
            std::path::Path::new(&dir)
                .join("separate")
                .to_string_lossy()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_continue_from_resets() {
        let dir = temp_dir("contf");
        setup_subtitle(&dir);

        run_workflow_engine(&dir, &EngineOptions::default()).unwrap();
        // 重跑后半段: separate_after 要重新执行, separate 不重跑
        run_workflow_engine(
            &dir,
            &EngineOptions {
                continue_from: Some("separate_after".to_string()),
                ..EngineOptions::default()
            },
        )
        .unwrap();

        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        let finished = |step: &str| {
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::StepFinished { step_id, .. } if step_id == step))
                .count()
        };
        assert_eq!(finished("separate"), 1, "separate 不应重跑");
        assert_eq!(finished("separate_after"), 2, "separate_after 应续跑");

        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");
        assert_eq!(status_of(&dir, "separate"), StepStatus::Success);
        assert_eq!(status_of(&dir, "separate_after"), StepStatus::Success);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
