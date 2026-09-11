//! 用 workflow-core 引擎 (aa-workflow) 驱动 pipeline (Phase 1: 串行等价)。
//!
//! 目标: 与 [`crate::workflows::pipeline::run_pipeline`] 对同样的 stage 序列、同样的
//! ctx.json 副作用达成一致, 但"成败真相"从 ctx.json 转移为
//! `<workflow_dir>/workflow-engine/events.jsonl` (见 [`engine_store::FsRunStore`])。
//!
//! 建立方式: 每个活跃 stage (过滤掉无 handler 的) 建成一个 node, 依赖前一个已注册 node
//! (显式链式 DAG), `max_concurrency = 1` 强制串行。node 闭包内部复用
//! [`crate::workflows::pipeline::run_stage`], 前后照旧写 ctx.json (UI 投影)。
//!
//! 与 run_pipeline 的差异点 (引擎天然语义):
//! - **resume**: 同 run_id 重跑时跳过事件日志中已 `success` 的 node。
//! - **continue_from**: 把目标 stage 起的后缀 node 重置为 pending 再续跑
//!   (镜像 `continue_pipeline` 在 ctx.json 上的重置)。
//! - **target_stage**: 命中即停, 标记成功 (镜像 run_pipeline 的 targetStep)。
//! - 阶段错误 → 事件日志记 NodeFailed, engine 停, workflow 置 failed。

use std::sync::Arc;

use workflow_core::{run as engine_run, NodeSpec, RunCtx, RunOptions, RunStatus};

use crate::context::read_ctx;
use crate::stages::get_stages;
use crate::stages::utils::{now_iso, set_stage_anyhow, set_workflow_anyhow, video_id, StepPatch, StepStatus, WorkflowPatch};
use crate::workflows::pipeline::{has_handler, run_stage};

use super::engine_store::FsRunStore;

/// 引擎调用参数 (由调用方注入; 未显式给出的回退到 ctx.input.workflow 里的字段)。
#[derive(Default, Debug, Clone)]
pub struct EngineOptions {
    /// 从该 stage 起重置为 pending 续跑 (镜像 continue_pipeline 的 continueFrom)。
    pub continue_from: Option<String>,
    /// 命中该 stage 即停 (镜像 run_pipeline 的 targetStep)。
    pub target_stage: Option<String>,
}

impl EngineOptions {
    /// 从 `ctx.input` 填充: `workflow.continueFrom` / `workflow.targetStep`
    /// (TS 续跑路径放这); `run_pipeline` 的内联约定 `targetStep` 顶层字段也兜底读取。
    pub fn from_ctx_input(input: &serde_json::Value) -> Self {
        let wf = input.get("workflow");
        let get = |k: &str| wf.and_then(|v| v.get(k)).and_then(|v| v.as_str()).map(String::from);
        let target_stage = get("targetStep").or_else(|| input.get("targetStep").and_then(|v| v.as_str()).map(String::from));
        Self {
            continue_from: get("continueFrom"),
            target_stage,
        }
    }
}

/// 用引擎跑 (或续跑) 一条 pipeline, 结束后 workflow.status ∈ {success, failed}。
pub fn run_workflow_engine(workflow_dir: &str, opts: &EngineOptions) -> anyhow::Result<()> {
    let _guard = tracing::info_span!("workflow", workflow_dir = workflow_dir).entered();
    let ctx = read_ctx(workflow_dir).map_err(anyhow::Error::msg)?;
    let stages = get_stages(&ctx);

    let target_stage = opts
        .target_stage
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).target_stage);
    let continue_from = opts
        .continue_from
        .clone()
        .or_else(|| EngineOptions::from_ctx_input(&ctx.input).continue_from);

    // target_stage 不在序列中则告警忽略 (镜像 run_pipeline）
    if let Some(ts) = &target_stage {
        if !stages.iter().any(|s| s == ts) {
            tracing::info!(target: "engine",
                "[WARN] target_stage \"{ts}\" 不在 {} pipeline 中, 忽略", ctx.pipeline);
        }
    }

    // continue_from → 先把后缀阶段在 ctx.json 重置为 pending (镜像 continue_pipeline)
    if let Some(cf) = &continue_from {
        let idx = stages
            .iter()
            .position(|s| s == cf)
            .ok_or_else(|| anyhow::anyhow!("Unknown stage \"{cf}\""))?;
        for s in &stages[idx..] {
            set_stage_anyhow(
                workflow_dir,
                s,
                StepPatch { status: Some(StepStatus::Pending), ..Default::default() },
            )?;
        }
        tracing::info!(target: "engine",
            "Resetting from \"{cf}\" ({} stage(s))", stages.len() - idx);
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
    let run_id = video_id(workflow_dir).ok_or_else(|| anyhow::anyhow!("无法从 workflow_dir 取 run_id"))?;
    let mut wf = workflow_core::Workflow::new(ctx.pipeline.clone());
    let mut prev: Option<String> = None;
    for stage in &stages {
        if !has_handler(stage) {
            tracing::info!(target: "engine", "[WARN] No handler for stage {stage}, skipping");
            continue;
        }
        let dir = workflow_dir.to_string();
        let name = stage.clone();
        let node_name = name.clone();
        let mut spec = NodeSpec::new(name.clone(), Arc::new(move |_ctx: RunCtx| {
            tracing::info!(target: "engine", "Running {node_name}");
            set_stage_anyhow(
                &dir,
                &node_name,
                StepPatch {
                    status: Some(StepStatus::Running),
                    started_at: Some(now_iso()),
                    last_message: Some(format!("Starting {node_name}...")),
                    ..Default::default()
                },
            )?;
            set_workflow_anyhow(
                &dir,
                WorkflowPatch {
                    status: Some("running".to_string()),
                    current_stage: Some(Some(node_name.clone())),
                    ..Default::default()
                },
            )?;
            match run_stage(&node_name, &dir) {
                Ok(()) => Ok(None),
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!(target: "engine", "Step {node_name} failed: {msg}");
                    set_stage_anyhow(
                        &dir,
                        &node_name,
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
        }));
        if let Some(p) = &prev {
            spec = spec.needs([p.clone()]);
        }
        prev = Some(name);
        wf = wf.node(spec);
    }

    let store = FsRunStore::new(workflow_dir);
    let mut r_opts = RunOptions::new(ctx.input.clone())
        .run_id(run_id.clone())
        .max_concurrency(1);
    if let Some(cf) = &continue_from {
        r_opts = r_opts.continue_from(cf.clone());
    }
    if let Some(ts) = &target_stage {
        r_opts = r_opts.target_stage(ts.clone());
    }

    let outcome = engine_run(&mut wf, &store, &r_opts, None)
        .map_err(|e| anyhow::anyhow!("workflow engine error: {e}"))?;

    match outcome.status {
        RunStatus::Finished => {
            set_workflow_anyhow(
                workflow_dir,
                WorkflowPatch {
                    status: Some("success".to_string()),
                    completed_at: Some(now_iso()),
                    current_stage: Some(None),
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
            "stages": {
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
        })).unwrap();
        ctx.workflow.workflow_dir = dir.to_string();
        ctx.workflow.id = "t".to_string();
        ctx.pipeline = "subtitle".to_string();
        write_ctx(dir, &ctx).unwrap();
    }

    fn status_of(dir: &str, stage: &str) -> StepStatus {
        let ctx = read_ctx(dir).unwrap();
        ctx.stages
            .unwrap()
            .iter()
            .find(|s| s.name == stage)
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
            assert_eq!(ctx.workflow.current_stage, None);
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

        // 事件日志: 每个 node 只跑一次 → 只应有 1 次 NodeFinished
        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        let n_finished = |stage: &str| {
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::NodeFinished { node_id, .. } if node_id == stage))
                .count()
        };
        assert_eq!(n_finished("separate"), 1);
        assert_eq!(n_finished("separate_after"), 1);

        // 再次运行 → 全部命中事件日志 success, 无 node 重跑
        run_workflow_engine(&dir, &EngineOptions::default()).unwrap();
        let events = store.get_events("t").unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::NodeFinished { node_id, .. } if node_id == "separate"))
                .count(),
            1
        );
        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_target_stage_stops() {
        let dir = temp_dir("target");
        setup_subtitle(&dir);

        run_workflow_engine(
            &dir,
            &EngineOptions {
                target_stage: Some("separate".to_string()),
                ..EngineOptions::default()
            },
        )
        .unwrap();

        let ctx = read_ctx(&dir).unwrap();
        assert_eq!(ctx.workflow.status, "success");
        assert_eq!(status_of(&dir, "separate"), StepStatus::Success);
        // separate_after 未跑 → ctx.json 无对应 stage 条目 (或非 success)
        let after = ctx
            .stages
            .as_ref()
            .and_then(|s| s.iter().find(|s| s.name == "separate_after"));
        assert!(after.map(|s| s.status != StepStatus::Success).unwrap_or(true));

        // 事件日志里 separate_after 不应有 NodeFinished
        let store = FsRunStore::new(&dir);
        let events = store.get_events("t").unwrap();
        assert!(
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::NodeFinished { .. }))
                .all(|e| e.node_id() != Some("separate_after")),
            "separate_after 不应被引擎执行"
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
        let finished = |stage: &str| {
            events
                .iter()
                .filter(|e| matches!(e, workflow_core::RunEvent::NodeFinished { node_id, .. } if node_id == stage))
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