//! 引擎路径（start）——core 直驱 aa-workflow，复用 [`FsRunStore`]。
//!
//! 与过渡的 `pipeline` / `parallel` 路径互斥（见 `exec_path`）：这里是唯一
//! 真源走事件日志（`workflow-store/<run_id>/logs.jsonl`）的路径，`view.json`
//! 等状态文件在此路径下不写。
//!
//! run_id 按文件名自增：扫 `<video_dir>/workflow-store/` 下的数字目录名取
//! 最大 +1（`1` / `2` / …）。目录存在即记录，重启可续；整体清理后从 1 重来。
//! continue / resume 不在本模块范围（留 TODO，见 INTEGRATION-PLAN）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aa_workflow_core::{RunStatus, RunWorkflowOptions, run_workflow};
use anyhow::{Context, bail};

use crate::input::WorkflowInput;
use crate::workflow::dub_sf_ocr;

use super::engine_store::FsRunStore;

/// `<video_dir>/workflow-store/` 下数字目录名的最大值 +1；目录不存在则从 1 开始。
fn next_run_id(base: &Path) -> String {
    let max = std::fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    (max + 1).to_string()
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
    let input_value = serde_json::to_value(wfi).context("序列化 workflow input 失败")?;

    tracing::info!(target: "output_mode", run_id = %run_id, "引擎路径启动 (dub-sf-ocr)");
    let outcome = run_workflow(
        RunWorkflowOptions::new(dub_sf_ocr(), store)
            .run_id(run_id.clone())
            .input(input_value),
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
