//! subtitle × asr 流程的 aa-workflow 声明（代码即 DAG）。
//!
//! 7 节点：asr/asr_fix 提文本 → translate → split_audio（仅取时序）→ mix_video 烧字幕。
//! 无 tts / mix_audio（不配音）。产物走盘上文件，`deps` 只表达顺序。

use std::sync::Arc;

use aa_workflow_core::{CreateWorkflowConfig, WorkflowDefinition, create_workflow};
use aa_workflow_middlewares::{DagSpec, dag_to_handler, define_dag};
use serde_json::Value;

use crate::input::WorkflowInput;
use crate::steps::asr::args::AsrDeps;
use crate::steps::asr::fix_args::AsrFixDeps;
use crate::steps::asr::fix::step_asr_fix;
use crate::steps::asr::step_asr;
use crate::steps::mix_video::args::MixVideoDeps;
use crate::steps::mix_video::step_mix_video;
use crate::steps::separate::{
    after_args::SeparateAfterDeps, step_separate, step_separate_after,
};
use crate::steps::split_audio::args::SplitAudioDeps;
use crate::steps::split_audio::step_split_audio;
use crate::steps::translate::args::TranslateDeps;
use crate::steps::translate::step_translate;

fn build_spec() -> DagSpec<WorkflowInput, ()> {
    DagSpec::new("subtitle-asr")
        .node("separate", Vec::<&str>::new(), |sc, _deps| async move {
            step_separate(sc).await?;
            Ok(Value::Null)
        })
        .node("separate_after", ["separate"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_separate_after(sc, SeparateAfterDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("asr", Vec::<&str>::new(), |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_asr(sc, AsrDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("asr_fix", ["asr"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_asr_fix(sc, AsrFixDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("translate", ["asr_fix"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_translate(sc, TranslateDeps::from_input(&wfi)?).await?;
            Ok(Value::Null)
        })
        .node(
            "split_audio",
            ["translate", "separate"],
            |sc, _deps| async move {
                let wfi = sc.ctx.input.clone();
                step_split_audio(sc, SplitAudioDeps::from_input(&wfi)?).await?;
                Ok(Value::Null)
            },
        )
        .node(
            "mix_video",
            ["translate", "split_audio", "separate_after"],
            |sc, _deps| async move {
                let wfi = sc.ctx.input.clone();
                step_mix_video(sc, MixVideoDeps::from_input(&wfi)?).await?;
                Ok(Value::Null)
            },
        )
}

pub fn subtitle_asr_spec() -> anyhow::Result<Arc<DagSpec<WorkflowInput, ()>>> {
    define_dag(build_spec())
}

pub fn subtitle_asr_with_policies(
    get_events: Arc<aa_workflow_middlewares::GetEvents>,
) -> anyhow::Result<aa_workflow_core::WorkflowDefinition<WorkflowInput, Value, ()>> {
    use aa_workflow_middlewares::dag_policies;
    let spec = subtitle_asr_spec()?;
    let shape = Arc::new(spec.shape().map_err(|e| anyhow::anyhow!("DAG 形状非法: {e}"))?);
    Ok(create_workflow(
        CreateWorkflowConfig::new("subtitle-asr")
            .input::<WorkflowInput>()
            .state::<()>()
            .initialize(|input: &WorkflowInput| {
                anyhow::ensure!(
                    input.workflow.video_dir.as_deref().map_or(false, |s| !s.is_empty()),
                    "workflow.video_dir 缺失"
                );
                Ok(())
            }),
    )
    .middleware(dag_policies(shape, get_events))
    .handler(dag_to_handler(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_asr_spec_is_valid() {
        let spec = subtitle_asr_spec().expect("静态边表应合法");
        assert_eq!(spec.nodes.len(), 7);
        assert!(!spec.nodes.contains_key("tts"));
        assert!(!spec.nodes.contains_key("mix_audio"));
        assert_eq!(
            spec.nodes["mix_video"].deps,
            vec!["translate", "split_audio", "separate_after"]
        );
    }
}
