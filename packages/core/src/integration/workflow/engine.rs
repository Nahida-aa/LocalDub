//! 引擎路径（start / resume）——core 直驱 aa-workflow，复用 [`FsRunStore`]。
//!
//! start / resume 的唯一执行路径：引擎真源是 append-only 事件日志
//! `workflow-store/<run_id>/logs.jsonl`。`view.json` / `<step>/state.json`
//! 仍由各 step handler 内部写（步骤间传参 / UI 投影用），但不是成败真源。
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

/// 驱动一次引擎 run，按 status 判定结果。
///
/// `goals_pause_ok`: 设了 `target_step`（= DagPlan.goals）时，目标步骤全部达成后
/// 中间件会主动泊车（`SIGNAL_AWAITED __interim-goal`），run 以 `Paused` 结束——
/// 这是**预期成功**，不是失败。LocalDub 的 DAG 没有 approval/sleep 等其它挂起源，
/// 故 goals 非空时的 Paused 可安全视为目标达成。
async fn drive(
    wf: aa_workflow_core::AnyWorkflowDefinition,
    store: Arc<dyn aa_workflow_core::RunStore>,
    run_id: &str,
    input: &WorkflowInput,
    meta: MetaBag,
    goals_pause_ok: bool,
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
        RunStatus::Paused if goals_pause_ok => {
            tracing::info!(target: "output_mode", run_id = %run_id, "引擎 run 目标达成，泊车等待");
            Ok(())
        }
        other => bail!("引擎 run {run_id} 未正常完成: {other:?}"),
    }
}

/// 按 (output_mode, subtitle_source) 选对应的 DAG 定义（6 种工作流各有独立 spec）。
///
/// strategy 中间件在 `*_with_policies` 里统一挂好；start/resume 都用这个定义，
/// 区别只在传入的 DagPlan meta（start 不传）。
fn build_workflow(wfi: &WorkflowInput) -> anyhow::Result<aa_workflow_core::AnyWorkflowDefinition> {
    use crate::workflow::args::{OutputMode, SubtitleSource};
    use crate::workflow::workflows::{dub_asr, dub_asr_ocr, dub_sf_ocr, subtitle_asr, subtitle_asr_ocr, subtitle_sf_ocr};

    let mode = wfi.workflow.output_mode;
    let source = wfi.workflow.subtitle_source;
    let wf = match (mode, source) {
        (OutputMode::Dub, SubtitleSource::SfOcr) => dub_sf_ocr::dub_sf_ocr_with_policies()?,
        (OutputMode::Dub, SubtitleSource::Asr) => dub_asr::dub_asr_with_policies()?,
        (OutputMode::Dub, SubtitleSource::AsrOcr) => dub_asr_ocr::dub_asr_ocr_with_policies()?,
        (OutputMode::Subtitle, SubtitleSource::Asr) => subtitle_asr::subtitle_asr_with_policies()?,
        (OutputMode::Subtitle, SubtitleSource::SfOcr) => subtitle_sf_ocr::subtitle_sf_ocr_with_policies()?,
        (OutputMode::Subtitle, SubtitleSource::AsrOcr) => subtitle_asr_ocr::subtitle_asr_ocr_with_policies()?,
    };
    Ok(Arc::new(wf).into())
}

/// 工作流的展示名（日志用）。
fn workflow_label(wfi: &WorkflowInput) -> String {
    format!(
        "{} × {}",
        wfi.workflow.output_mode.as_str(),
        wfi.workflow.subtitle_source.as_str()
    )
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
    let wf = build_workflow(wfi)?;

    tracing::info!(target: "output_mode", run_id = %run_id, workflow = %workflow_label(wfi), "引擎路径启动");
    drive(wf, store, &run_id, wfi, MetaBag::new(), false).await
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
    let wf = build_workflow(wfi)?;

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
        run_id = %run_id, source = %source_run_id, workflow = %workflow_label(wfi), "引擎 resume 启动");
    // target_step 非空 = goals 已设，目标达成后的 Paused 是预期成功。
    let goals_pause_ok = !wfi.workflow.target_step.is_empty();
    drive(wf, store, &run_id, wfi, meta, goals_pause_ok).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wfi(mode: &str, source: &str) -> WorkflowInput {
        let raw = format!(
            r#"{{"workflow":{{"output_mode":"{mode}","subtitle_source":"{source}","video_dir":"/tmp/x"}},"steps":{{}}}}"#
        );
        let input: crate::input::Input = serde_json::from_str(&raw).unwrap();
        WorkflowInput::try_from(input).unwrap()
    }

    #[test]
    fn all_six_workflows_build_engine_definitions() {
        // 6 种 (output_mode × subtitle_source) 各有独立 DAG，都能构建出定义。
        for (mode, source) in [
            ("dub", "sf_ocr"),
            ("dub", "asr"),
            ("dub", "asr_ocr"),
            ("subtitle", "asr"),
            ("subtitle", "sf_ocr"),
            ("subtitle", "asr_ocr"),
        ] {
            let w = wfi(mode, source);
            build_workflow(&w)
                .unwrap_or_else(|e| panic!("{mode}+{source} 应能构建引擎定义: {e}"));
        }
    }
}
