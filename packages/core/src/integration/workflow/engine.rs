//! 引擎路径（start / resume）——core 直驱 aa-workflow，复用 [`FsRunStore`]。
//!
//! 与过渡的 `pipeline` / `parallel` 路径互斥（见 `exec_path`）：这里是唯一
//! 真源走事件日志（`workflow-store/<run_id>/logs.jsonl`）的路径，`view.json`
//! 等状态文件在此路径下不写。
//!
//! run_id 按文件名自增：扫 `<video_dir>/workflow-store/` 下的数字目录名取
//! 最大 +1（`1` / `2` / …）。目录存在即记录，重启可续；整体清理后从 1 重来。
//!
//! resume 走引擎主线范式：失效重跑靠**新 run_id + source_run_id**（同 run_id
//! 重驱会被日志里的 STEP_FINISHED 短路，无法让步骤真重跑）。新 run 从源 run
//! 导入「未失效步骤」的结果，失效步骤（DagPlan.from / 缺省未成功集）真跑。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aa_workflow_core::define::MetaBag;
use aa_workflow_core::{RunStatus, RunWorkflowOptions, run_workflow};
use aa_workflow_middlewares::{DagPlan, DagPlanKey};
use anyhow::{Context, bail};

use crate::input::WorkflowInput;
use crate::workflow::workflows::dub_sf_ocr as dub_sf_ocr_mod;

use super::engine_store::FsRunStore;

/// `<video_dir>/workflow-store/` 下数字目录名列表（升序）。
fn existing_run_ids(base: &Path) -> Vec<u64> {
    let mut ids: Vec<u64> = std::fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u64>().ok())
        .collect();
    ids.sort_unstable();
    ids
}

/// 新 run_id = 现有最大 +1；目录不存在则从 1 开始。
fn next_run_id(base: &Path) -> String {
    (existing_run_ids(base).last().copied().unwrap_or(0) + 1).to_string()
}

/// 构造读源 run 日志的闭包（供 dag_policies 中间件做 reseed 复用）。
fn get_events_loader(store: Arc<dyn aa_workflow_core::RunStore>) -> Arc<aa_workflow_middlewares::GetEvents> {
    Arc::new(move |run_id: &str| {
        use aa_workflow_core::define::BoxFuture;
        use aa_workflow_core::RunStore;
        let store = store.clone();
        Box::pin(async move { Ok(store.get_events(run_id).await?) }) as BoxFuture<'_, _>
    })
}

/// 驱动一次引擎 run，按 status 判定结果。
async fn drive(
    wf: aa_workflow_core::AnyWorkflowDefinition,
    store: Arc<dyn aa_workflow_core::RunStore>,
    run_id: &str,
    input: &WorkflowInput,
    meta: MetaBag,
) -> anyhow::Result<()> {
    let input_value = serde_json::to_value(input).context("序列化 workflow input 失败")?;
    let outcome = run_workflow(
        RunWorkflowOptions::new(wf, store)
            .run_id(run_id.to_string())
            .input(input_value)
            .meta(meta),
    )
    .outcome()
    .await
    .map_err(|e| anyhow::anyhow!("引擎 run {run_id} 失败: {e:?}"))?;

    match outcome.status {
        RunStatus::Finished => {
            tracing::info!(target: "output_mode", run_id = %run_id, "引擎 run 完成");
            Ok(())
        }
        other => bail!("引擎 run {run_id} 未正常完成: {other:?}"),
    }
}

/// 驱动一次全新的引擎 run（start 路径）。
///
/// `wfi` 是 import 已回填身份的 run input。store 与 run_id 都在 async 上下文里
/// 构造（`FsRunStore::new` 内部 `tokio::spawn` 要求 runtime）。
pub async fn run_engine(wfi: &WorkflowInput) -> anyhow::Result<()> {
    let video_dir = PathBuf::from(wfi.video_dir());
    let store_base = video_dir.join("workflow-store");
    let run_id = next_run_id(&store_base);

    let store: Arc<dyn aa_workflow_core::RunStore> = Arc::new(FsRunStore::new(&store_base));
    let wf: aa_workflow_core::AnyWorkflowDefinition =
        Arc::new(dub_sf_ocr_mod::dub_sf_ocr_with_policies(get_events_loader(store.clone()))?).into();

    tracing::info!(target: "output_mode", run_id = %run_id, "引擎路径启动 (dub-sf-ocr)");
    drive(wf, store, &run_id, wfi, MetaBag::new()).await
}

/// 驱动一次引擎 resume（新 run + DagPlan 从源 run 复用未失效步骤）。
///
/// `wfi.workflow.resume_from` 直接透传为 DagPlan.from（空 → None，引擎自动取
/// 源日志里未成功的节点为失效种子）；`target_step` 透传为 goals。
pub async fn run_engine_resume(wfi: &WorkflowInput) -> anyhow::Result<()> {
    let video_dir = PathBuf::from(wfi.video_dir());
    let store_base = video_dir.join("workflow-store");
    let source = existing_run_ids(&store_base).last().copied();
    let source_run_id = source.ok_or_else(|| anyhow::anyhow!("resume 失败：没有可复用的源 run 日志"))?;
    let run_id = (source_run_id + 1).to_string();
    let source_run_id = source_run_id.to_string();

    let store: Arc<dyn aa_workflow_core::RunStore> = Arc::new(FsRunStore::new(&store_base));
    let wf: aa_workflow_core::AnyWorkflowDefinition =
        Arc::new(dub_sf_ocr_mod::dub_sf_ocr_with_policies(get_events_loader(store.clone()))?).into();

    let from: Vec<String> = wfi
        .workflow
        .resume_from
        .iter()
        .map(|s| s.as_str().to_string())
        .collect();
    let goals: Vec<String> = wfi
        .workflow
        .target_step
        .iter()
        .map(|s| s.as_str().to_string())
        .collect();
    let plan = DagPlan {
        source_run_id: Some(source_run_id.clone()),
        from: if from.is_empty() { None } else { Some(from) },
        redo_set: None,
        goals,
    };
    let meta = MetaBag::new().with::<DagPlanKey>(plan);

    tracing::info!(target: "output_mode",
        run_id = %run_id, source = %source_run_id, "引擎 resume 启动 (dub-sf-ocr)");
    drive(wf, store, &run_id, wfi, meta).await
}
