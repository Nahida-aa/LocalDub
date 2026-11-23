//! 并行执行器（实验性并行，`workflow.parallel = true` / CLI `--parallel` 启用）：
//! 按 [`StepDag`] 调度，
//! **不引入 workflow 引擎**——就绪步骤起 OS 线程（每步是阻塞子进程编排，
//! 线程即够），失败等全部在跑的步骤收尾再汇总报错。
//!
//! 为什么并行在这里是安全的：单写者改造之后每个步骤只写自己的
//! `<step>/state.json` / `<step>/out.json`；本执行器**不在步骤间**写
//! `state.json`（workflow 级状态只在 run 边界写一次）——并行分支互不踩。
//!
//! 与引擎路径（`integration::workflow`）的区别：无事件日志、无 replay、
//! 无 pause/approve。实验性质，真跑验证过再决定要不要把引擎接进来。
//!
//! ## 失败语义（与用户裁决一致）
//!
//! 一分支失败**不取消**另一分支——每步都是 spawn 子进程的阻塞调用，
//! "取消"意味着杀对方分支的子进程，复杂且脏。等全部收尾后汇总：
//! 报第一个失败 + 其余失败 + 因上游失败而未运行的步骤清单。
//!
//! ## 续跑语义（与用户裁决一致）
//!
//! **按步跳过**：`action=continue` 时每步跑前读自己的 `<step>/state.json`，
//! `Success` 就跳过——半途失败后续跑不会重跑另一分支已完成的步骤
//! （串行 `continue_pipeline` 的前缀扫描做不到这一点）。
//! `continue_from` 显式重跑：该步 + 传递下游重置 pending。
//!
//! ## target_step
//!
//! 只调度 `target` 及其传递上游（DAG 的自然子图），比串行版"命中即停"
//! 更精确——无关分支根本不开跑。

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::input::Input;
use crate::state::StepStatus;
use crate::steps::utils::{
    now_iso, set_step_anyhow, set_workflow_anyhow, StepPatch, WorkflowPatch,
};
use crate::workflow::args::{StepName, WorkflowAction};
use crate::workflow::dag::StepDag;
use crate::workflow::pipeline::run_one_step;

/// DAG 调度核心：就绪（依赖全部 Ok 且在 `allowed` 内）即起 tokio task 跑
/// `runner`；失败不取消在跑的分支，全部收尾后统一报错。
///
/// `skip` 里的步骤视为已成功（续跑跳过已完成步骤）；
/// `allowed` 限定调度范围（target_step 子图；`None` = 全图，图外节点不动）。
///
/// 步骤是 async I/O 等待，用 tokio task 而非 OS 线程——等待时不占线程。
async fn run_dag<F, Fut>(
    dag: &StepDag,
    skip: &BTreeSet<StepName>,
    allowed: Option<&BTreeSet<StepName>>,
    runner: F,
) -> Result<(), String>
where
    F: Fn(StepName) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum St {
        Pending,
        Running,
        Ok,
        Failed,
    }

    let mut state: BTreeMap<StepName, St> = dag.nodes().iter().map(|s| (*s, St::Pending)).collect();
    for s in skip {
        if let Some(st) = state.get_mut(s) {
            *st = St::Ok;
        }
    }
    let mut errors: Vec<(StepName, String)> = Vec::new();

    let (tx, mut rx) = mpsc::unbounded_channel::<(StepName, anyhow::Result<()>)>();
    let runner = Arc::new(runner);
    loop {
        // 调度所有就绪步骤（依赖全部 Ok 且在 allowed 内）
        for step in dag.nodes() {
            if state[step] != St::Pending {
                continue;
            }
            if let Some(allowed) = allowed {
                if !allowed.contains(step) {
                    continue;
                }
            }
            if !dag.deps(*step).iter().all(|d| state[d] == St::Ok) {
                continue;
            }
            state.insert(*step, St::Running);
            let tx = tx.clone();
            let runner = Arc::clone(&runner);
            let step = *step;
            tokio::spawn(async move {
                let r = runner(step).await;
                let _ = tx.send((step, r));
            });
        }
        // 无在跑 task = 没有可推进的了（剩余 Pending 全是被失败/范围挡住的）
        if !state.values().any(|s| *s == St::Running) {
            break;
        }
        match rx.recv().await {
            Some((step, r)) => {
                state.insert(step, if r.is_ok() { St::Ok } else { St::Failed });
                if let Err(e) = r {
                    errors.push((step, e.to_string()));
                }
            }
            None => break, // 发送端全关（task 全 panic 的兜底）
        }
    }

    // 汇报范围：allowed 内仍 Pending 的都是被上游失败挡住的；范围外的本来就
    // 不归这次运行管（target_step 子图之外）。
    let report_set: &BTreeSet<StepName> = allowed.unwrap_or(dag.nodes());
    let blocked: Vec<String> = report_set
        .iter()
        .filter(|s| state[*s] == St::Pending)
        .map(|s| s.as_str().to_string())
        .collect();

    if !errors.is_empty() {
        let mut msg = errors
            .iter()
            .map(|(s, e)| format!("步骤 {} 失败: {e}", s.as_str()))
            .collect::<Vec<_>>()
            .join("; ");
        if !blocked.is_empty() {
            msg.push_str(&format!("；因上游失败未运行: {}", blocked.join(", ")));
        }
        return Err(msg);
    }
    if !blocked.is_empty() {
        return Err(format!(
            "步骤从未被调度（依赖未满足，疑似 DAG 定义有缺口）: {}",
            blocked.join(", ")
        ));
    }
    Ok(())
}

/// 按 DAG 并行跑一个 output_mode（镜像 `run_pipeline` 的并行版）。
///
/// 入口语义与 [`crate::workflow::pipeline::run_pipeline`] 对齐：
/// `action=start` 全新全量跑；`action=continue` 按步跳过已 success 的步骤，
/// `continue_from` 显式重跑该步及其传递下游。
/// 任务目录取自 `input.workflow.video_dir`（[`crate::input::Input::video_dir`]）。
pub async fn run_pipeline_parallel(input: &Input) -> anyhow::Result<()> {
    let video_dir = input.video_dir()?;
    // 进入 workflow span: 携带 video_dir 供 WorkflowFileLayer 落盘。
    let _workflow_guard = tracing::info_span!("workflow", video_dir = video_dir).entered();
    tracing::info!(target: "output_mode", "run_pipeline_parallel: start");

    let dag = StepDag::for_input(input)?;
    let wf = input.workflow.as_ref();
    let is_continue = wf.and_then(|w| w.action) == Some(WorkflowAction::Continue);

    // continue_from: 该步 + 传递下游重置 pending（skip-if-success 不再跳过它们）。
    if is_continue {
        if let Some(cf) = wf.and_then(|w| w.continue_from) {
            for step in std::iter::once(cf).chain(dag.transitive_dependents(cf)) {
                set_step_anyhow(
                    video_dir,
                    step.as_str(),
                    StepPatch {
                        status: Some(StepStatus::Pending),
                        ..Default::default()
                    },
                )?;
            }
            tracing::info!(target: "output_mode", "continue_from \"{}\": 已重置该步骤及其传递下游", cf.as_str());
        }
    }

    // 续跑：已 success 的步骤跳过。start 不跳——同目录重跑应全量重跑（与串行一致）。
    let skip: BTreeSet<StepName> = if is_continue {
        dag.nodes()
            .iter()
            .filter(|s| {
                crate::state::read_step_state(video_dir, s.as_str())
                    .map(|st| st.status == StepStatus::Success)
                    .unwrap_or(false)
            })
            .copied()
            .collect()
    } else {
        BTreeSet::new()
    };
    for s in &skip {
        tracing::info!(target: "output_mode", "{} 已 success, 跳过", s.as_str());
    }

    let allowed = match wf.and_then(|w| w.target_step) {
        Some(t) => Some(dag.with_target_closure(t)?),
        None => None,
    };

    set_workflow_anyhow(
        video_dir,
        WorkflowPatch {
            status: Some("running".to_string()),
            started_at: Some(now_iso()),
            ..Default::default()
        },
    )?;

    // EnteredSpan 是 !Send，不能跨 .await 持有（run_dag 内部 tokio::spawn 要求 Send）；
    // 进入并发段前退出 span，各 step 自己的 span 由 run_one_step 重建。
    drop(_workflow_guard);

    // runner 必须 'static（tokio::spawn），故给它单独的 owning clone；
    // 原 video_dir 留给下面写 workflow 总结状态用。
    let runner_video_dir = video_dir.to_string();
    let runner_input = input.clone();
    let runner = move |step: StepName| {
        let video_dir = runner_video_dir.clone();
        let input = runner_input.clone();
        async move { run_one_step(&video_dir, step, &input).await }
    };
    match run_dag(&dag, &skip, allowed.as_ref(), runner).await {
        Ok(()) => {
            set_workflow_anyhow(
                video_dir,
                WorkflowPatch {
                    status: Some("success".to_string()),
                    completed_at: Some(now_iso()),
                    current_step: Some(None),
                    ..Default::default()
                },
            )?;
            tracing::info!(target: "output_mode", "run_pipeline_parallel: success");
            Ok(())
        }
        Err(msg) => {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use StepName::{Asr, AsrFix, Translate};

    /// 测试 DAG：asr → asr_fix，translate 独立。
    fn test_dag() -> StepDag {
        StepDag::new(
            [Asr, AsrFix, Translate],
            [(Asr, vec![]), (AsrFix, vec![Asr]), (Translate, vec![])],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn runs_all_nodes_respecting_deps() {
        let dag = test_dag();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let r = ran.clone();
        run_dag(&dag, &BTreeSet::new(), None, move |step| {
            let r = r.clone();
            async move {
            // asr 多睡一会，若 asr_fix 被提前调度会排在它前面
            if step == Asr {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            r.lock().unwrap().push(step);
            Ok(())
            }
        })
        .await
        .unwrap();

        let ran = ran.lock().unwrap().clone();
        assert_eq!(ran.len(), 3, "全部节点都应运行");
        let pos = |s: StepName| ran.iter().position(|x| *x == s).unwrap();
        assert!(pos(Asr) < pos(AsrFix), "asr_fix 必须晚于 asr");
    }

    /// **并行的实质证据**：两个无依赖的步骤必须重叠执行——asr 起跑后要"看见"
    /// translate 已经在跑。若调度器退化为串行，asr 等待期间 translate 不会启动，
    /// 此断言会红。
    #[tokio::test]
    async fn independent_steps_run_concurrently() {
        let dag = test_dag();
        let translate_started = Arc::new(AtomicBool::new(false));
        let overlap_seen = Arc::new(AtomicBool::new(false));

        let ts = translate_started.clone();
        let ov = overlap_seen.clone();
        run_dag(&dag, &BTreeSet::new(), None, move |step| {
            let ts = ts.clone();
            let ov = ov.clone();
            async move {
            match step {
                Asr => {
                    // 等 translate 起跑（最多 2s），看见即证明两线程重叠
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < deadline {
                        if ts.load(Ordering::SeqCst) {
                            ov.store(true, Ordering::SeqCst);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                }
                Translate => {
                    ts.store(true, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
                _ => {}
            }
            Ok(())
            }
        })
        .await
        .unwrap();

        assert!(
            overlap_seen.load(Ordering::SeqCst),
            "asr 与 translate 应并行重叠（调度器退化成串行了）"
        );
    }

    /// 失败语义：等全部收尾再报错——依赖失败的步骤**不运行**，
    /// 无关分支照常跑完，错误汇总在一条消息里。
    #[tokio::test]
    async fn failure_blocks_dependents_but_independent_completes() {
        let dag = test_dag();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let r = ran.clone();
        let err = run_dag(&dag, &BTreeSet::new(), None, move |step| {
            let r = r.clone();
            async move {
            r.lock().unwrap().push(step);
            if step == Asr {
                Err(anyhow::anyhow!("boom"))
            } else {
                Ok(())
            }
            }
        })
        .await
        .unwrap_err();

        let ran = ran.lock().unwrap().clone();
        assert!(ran.contains(&Asr));
        assert!(ran.contains(&Translate), "无关分支应照常跑完");
        assert!(!ran.contains(&AsrFix), "依赖失败的步骤不应运行");
        assert!(
            err.contains("asr") && err.contains("boom"),
            "错误应点名失败步骤: {err}"
        );
        assert!(
            err.contains("asr_fix"),
            "错误应列出因上游失败未运行的步骤: {err}"
        );
    }

    /// 续跑语义：skip 里的步骤不跑，其下游照常跑（视为已满足）。
    #[tokio::test]
    async fn skip_marks_steps_done() {
        let dag = test_dag();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let r = ran.clone();
        let skip = BTreeSet::from([Asr]);
        run_dag(&dag, &skip, None, move |step| {
            let r = r.clone();
            async move {
            r.lock().unwrap().push(step);
            Ok(())
            }
        })
        .await
        .unwrap();

        let ran = ran.lock().unwrap().clone();
        assert!(!ran.contains(&Asr), "skip 的步骤不应运行");
        assert!(ran.contains(&AsrFix), "skip 步骤的下游应正常调度");
        assert!(ran.contains(&Translate));
    }

    /// target_step 语义：只调度子图内的节点，图外节点不动且不算 blocked。
    #[tokio::test]
    async fn allowed_restricts_schedule_scope() {
        let dag = test_dag();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let r = ran.clone();
        let allowed = BTreeSet::from([Asr, AsrFix]);
        run_dag(&dag, &BTreeSet::new(), Some(&allowed), move |step| {
            let r = r.clone();
            async move {
            r.lock().unwrap().push(step);
            Ok(())
            }
        })
        .await
        .unwrap();

        let ran = ran.lock().unwrap().clone();
        assert_eq!(ran.len(), 2, "只应运行 allowed 子图内的节点");
        assert!(!ran.contains(&Translate));
    }
}
