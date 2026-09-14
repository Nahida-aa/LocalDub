//! 用 workflow-core 引擎 (aa-workflow) 驱动 pipeline (Phase 1: 串行等价)。
//!
//! 目标: 与 [`crate::workflows::pipeline::run_pipeline`] 对同样的 step 序列、同样的
//! ctx.json 副作用达成一致, 但"成败真相"从 ctx.json 转移为
//! `<video_dir>/workflow-engine/events.jsonl` (见 [`engine_store::FsRunStore`])。
//!
//! 建立方式: 内核是 async handler-replay (代码即 DAG)——handler 闭包按 `get_steps`
//! 顺序逐个 `ctx.step(...).await`, 重放时已 success 的 step 短路到缓存结果、
//! 已失败的直接 rethrow。串行即 `for` 循环, 后续要并行再上 `tokio::try_join!`。
//! step 闭包内部复用 [`crate::workflows::pipeline::run_step`], 前后照旧写 ctx.json
//! (UI 投影)。
//!
//! 与 run_pipeline 的差异点 (引擎天然语义):
//! - **resume**: 同 run_id 重跑时跳过事件日志中已 `success` 的 step。
//! - **continue_from**: 事件日志在 store 层截断到目标 step 的最新终态 checkpoint,
//!   前缀短路、后缀重跑 (镜像 `continue_pipeline` 在 ctx.json 上的重置)。
//! - **target_step**: 命中即停, 标记成功 (镜像 run_pipeline 的 targetStep)。
//! - 阶段错误 → 事件日志记 StepFailed, engine 停, workflow 置 failed (失败即终局,
//!   重试靠 continue_from / 新 run)。

use std::sync::Arc;

use workflow_core::{run_workflow_sync, RunOptions, RunStatus, StepCtx, Workflow, WorkflowCtx};

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
fn step_artifacts(video_dir: &str, step: &str) -> Option<serde_json::Value> {
    use crate::steps::utils::{
        asr_dir, asr_ocr_dir, asr_ocr_fix_dir, asr_ocr_pre_dir, dubbing_path, final_video_dir,
        gated_vocals_path, mix_audio_timings_path, mixed_vocals_path, resolve_language,
        separate_dir, sf_ocr_dir, sf_ocr_fix_dir, sf_ocr_pre_dir, split_audio_path,
        split_audio_timings_path, subtitle_file_path, tts_filepath, video_id,
    };
    let wf = video_dir;
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
///
/// 入口是同步的 (`run_workflow_sync` 内部自建 tokio runtime 驱动 handler)。
pub fn run_workflow_engine(video_dir: &str, opts: &EngineOptions) -> anyhow::Result<()> {
    let _guard = tracing::info_span!("workflow", video_dir = video_dir).entered();
    let ctx = read_ctx(video_dir).map_err(anyhow::Error::msg)?;
    let steps = get_steps(&ctx);

    let target_step = opts
        .target_step
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).target_step);
    let continue_from = opts
        .continue_from
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).continue_from);

    // target_step 不在序列中则告警忽略 (镜像 run_pipeline)
    if let Some(ts) = &target_step {
        if !steps.iter().any(|s| s == ts) {
            tracing::info!(target: "engine",
                "[WARN] target_step \"{ts}\" 不在 {} pipeline 中, 忽略", ctx.pipeline);
        }
    }

    set_workflow_anyhow(
        video_dir,
        WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            ..Default::default()
        },
    )?;

    // continue_from 由 store 层截断事件日志承载: 重放时前缀短路、后缀重跑,
    // 不需要像 run_pipeline 那样提前把 ctx.json 后缀标 pending。

    // handler-replay authoring 面: 串行 = 按 get_steps 顺序逐个 ctx.step.await。
    let run_id =
        video_id(video_dir).ok_or_else(|| anyhow::anyhow!("无法从 video_dir 取 run_id"))?;
    let wf = Workflow::new(ctx.pipeline.clone()).handler({
        let dir = video_dir.to_string();
        let steps = steps;
        move |wctx: WorkflowCtx| {
            let dir = dir.clone();
            let steps = steps.clone();
            async move {
                for step in &steps {
                    if !has_handler(step) {
                        tracing::info!(target: "engine", "[WARN] No handler for step {step}, skipping");
                        continue;
                    }
                    let step_id = step.clone();
                    let step_id_c = step_id.clone();
                    let dir = dir.clone();
                    wctx.step(&step_id, move |_sc: StepCtx| {
                        let dir = dir.clone();
                        let step_name = step_id_c.clone();
                        async move {
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
                                Ok(()) => Ok(step_artifacts(&dir, &step_name)
                                    .unwrap_or(serde_json::Value::Null)),
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
                        }
                    })
                    .await?;
                }
                Ok(serde_json::Value::Null)
            }
        }
    });

    let store = FsRunStore::new(video_dir);
    let mut r_opts = RunOptions::new(ctx.input.clone()).run_id(run_id.clone());
    if let Some(cf) = &continue_from {
        r_opts = r_opts.continue_from(cf.clone());
    }
    if let Some(ts) = &target_step {
        r_opts = r_opts.target_step(ts.clone());
    }

    let outcome = run_workflow_sync(&wf, Arc::new(store), &r_opts, None)
        .map_err(|e| anyhow::anyhow!("workflow engine error: {e}"))?;

    apply_outcome(video_dir, &run_id, outcome)
}

/// 把引擎终态映射到 workflow 状态 + 返回值。
///
/// 单独抽出来是为了能直接测 `Paused` 那一支——pipeline 的 step handler 是固定的
/// 一组真实工序，没法顺手塞一个「会挂起」的进去。
fn apply_outcome(
    video_dir: &str,
    run_id: &str,
    outcome: workflow_core::RunOutcome,
) -> anyhow::Result<()> {
    match outcome.status {
        RunStatus::Finished => {
            set_workflow_anyhow(
                video_dir,
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
            // `outcome.error` 是结构化的 `RunError`（`Display` 转发 message）。
            let msg = outcome
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "unknown engine error".to_string());
            set_workflow_anyhow(
                video_dir,
                WorkflowPatch {
                    status: Some("failed".to_string()),
                    error_message: Some(msg.clone()),
                    ..Default::default()
                },
            )?;
            Err(anyhow::anyhow!(msg))
        }
        // ── 挂起（aa-workflow D3）────────────────────────────────────────
        //
        // core 跑到 `ctx.approve` / `ctx.sleep` / `wait_for_event` 时**写盘就返回**，
        // 不再原地等待（对齐上游 TanStack：`throw new WorkflowPaused()`）。
        // 所以「跑完一次 run_workflow_engine」不再等于「pipeline 跑完了」。
        //
        // 本仓的 pipeline 目前**不使用任何挂起原语**（handler 是纯 `ctx.step` 序列），
        // 所以正常路径下不会走到这里。走到这里说明：
        //   - 要么将来给某步加了 `approve`/`sleep`（那需要外部的投递者/驱动器）；
        //   - 要么引擎语义又变了（那说明这里的假设过期了）。
        // 两种情况都不该被静默当成成功或失败——如实报错，把问题暴露给调用方。
        // 见 aa-workflow `docs/runtime-design.md` D3。
        RunStatus::Paused => {
            let w = store_wait_desc(video_dir);
            set_workflow_anyhow(
                video_dir,
                WorkflowPatch {
                    status: Some("paused".to_string()),
                    current_step: Some(None),
                    ..Default::default()
                },
            )?;
            Err(anyhow::anyhow!(
                "workflow {run_id} 在挂起点停下，等待外部投递{w}；\
                 当前宿主没有投递者（pipeline 不带挂起原语，走到这里说明用例超出了引擎的\
                 当前支持范围）。见 aa-workflow docs/runtime-design.md D3"
            ))
        }
        RunStatus::Aborted => {
            set_workflow_anyhow(
                video_dir,
                WorkflowPatch {
                    status: Some("failed".to_string()),
                    error_message: Some("workflow aborted".to_string()),
                    ..Default::default()
                },
            )?;
            Err(anyhow::anyhow!("workflow {run_id} aborted"))
        }
        // `run_workflow` 的返回值只可能是上面四种终态；`Running` 不该出现。
        // 显式列出而不是留 `_`：将来 aa-workflow 加了新状态，这里会编译失败，
        // 逼我们回来决定它该怎么映射——而不是静默走进某个兜底分支。
        RunStatus::Running => Err(anyhow::anyhow!(
            "engine 返回了 Running 终态（不该发生）：run {run_id}"
        )),
    }
}

/// 挂起时把「在等什么」读出来，附进错误信息——`RunState` 信封里有投影。
fn store_wait_desc(video_dir: &str) -> String {
    let path = std::path::Path::new(video_dir)
        .join("workflow-engine")
        .join("run.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    let Ok(st) = serde_json::from_str::<workflow_core::RunState>(&raw) else {
        return String::new();
    };
    if let Some(w) = st.waiting_for {
        let step = w.step_id.unwrap_or_default();
        return format!("（等信号 \"{}\"，step \"{step}\"）", w.signal_name);
    }
    if let Some(pa) = st.pending_approval {
        return format!("（等审批 \"{}\"）", pa.approval_id);
    }
    String::new()
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
            "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": input,
            "pipeline": "subtitle"
        }))
        .unwrap();
        ctx.workflow.video_dir = dir.to_string();
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
                .filter(|e| matches!(e, workflow_core::WorkflowEvent::StepFinished { step_id, .. } if step_id == step))
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
                .filter(|e| matches!(e, workflow_core::WorkflowEvent::StepFinished { step_id, .. } if step_id == "separate"))
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
                .filter(|e| matches!(e, workflow_core::WorkflowEvent::StepFinished { .. }))
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
                workflow_core::WorkflowEvent::StepFinished {
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

        let store = FsRunStore::new(&dir);
        let get_sf_ts = |events: &[workflow_core::WorkflowEvent], step: &str| -> i64 {
            events
                .iter()
                .find_map(|e| match e {
                    workflow_core::WorkflowEvent::StepFinished {
                        step_id, ts, ..
                    } if step_id == step => Some(*ts),
                    _ => None,
                })
                .expect("应有 StepFinished")
        };
        let events_1 = store.get_events("t").unwrap();
        let (sep_first, sa_first) = (
            get_sf_ts(&events_1, "separate"),
            get_sf_ts(&events_1, "separate_after"),
        );

        // 续跑后半段: separate_after 要重新执行, separate 不重跑
        run_workflow_engine(
            &dir,
            &EngineOptions {
                continue_from: Some("separate_after".to_string()),
                ..EngineOptions::default()
            },
        )
        .unwrap();

        let store = FsRunStore::new(&dir);
        let events_2 = store.get_events("t").unwrap();
        // 截断语义: 后缀终态 checkpoint 删掉重记 → separate_after 的 ts 是新的;
        // 前缀 separate 未被触碰 → ts 保持 run1 的
        assert_eq!(
            get_sf_ts(&events_2, "separate"),
            sep_first,
            "separate 不应重跑"
        );
        assert_ne!(
            get_sf_ts(&events_2, "separate_after"),
            sa_first,
            "separate_after 应续跑并重记终态"
        );
        // 每个 suffix step 只留一条终态 (旧的已被截断)
        assert_eq!(
            events_2
                .iter()
                .filter(|e| matches!(e, workflow_core::WorkflowEvent::StepFinished { step_id, .. } if step_id == "separate_after"))
                .count(),
            1
        );

        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");
        assert_eq!(status_of(&dir, "separate"), StepStatus::Success);
        assert_eq!(status_of(&dir, "separate_after"), StepStatus::Success);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 挂起状态必须**如实报错**，不能静默当成成功或失败。
    ///
    /// pipeline 本身不带挂起原语（handler 是纯 `ctx.step` 序列），所以正常路径
    /// 走不到 `RunStatus::Paused`——直接调 `apply_outcome` 验那一支的映射。
    /// 先跑一个真会挂起的 workflow，拿到**真实的** `RunOutcome`（不是手搓的），
    /// 再喂给映射函数，这样两侧都覆盖到。见 aa-workflow D3。
    #[test]
    fn engine_reports_paused_instead_of_swallowing_it() {
        let dir = temp_dir("paused");
        setup_subtitle(&dir);

        // 真跑一个会挂起的 workflow：`approve` 写盘后立刻返回，不阻塞。
        let wf = Workflow::new("gate").handler(|wctx: WorkflowCtx| async move {
            wctx.approve("release", "manual gate").await?;
            Ok(serde_json::Value::Null)
        });
        let store = FsRunStore::new(&dir);
        let outcome = workflow_core::run_workflow_sync(
            &wf,
            Arc::new(store),
            &RunOptions::new(serde_json::json!({})).run_id("t"),
            None,
        )
        .unwrap();
        assert_eq!(outcome.status, RunStatus::Paused, "core 侧应挂起即返回");

        // 真实 outcome → 映射函数（就是 `run_workflow_engine` 用的那个）。
        let err = apply_outcome(&dir, "t", outcome)
            .expect_err("挂起必须报错，不能当成成功");

        let msg = err.to_string();
        assert!(
            msg.contains("挂起点") && msg.contains("release"),
            "错误信息应说明在等什么，实际: {msg}"
        );

        // workflow 状态如实置 paused（不是 success / failed）。
        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "paused");

        // 日志里不得出现终态事件——挂起不是终局。
        let events = FsRunStore::new(&dir).get_events("t").unwrap();
        assert!(
            !events.iter().any(|e| matches!(
                e,
                workflow_core::WorkflowEvent::RunFinished { .. }
                    | workflow_core::WorkflowEvent::RunErrored { .. }
            )),
            "挂起不得写终态事件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
