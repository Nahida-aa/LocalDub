//! subtitle × asr_ocr 流程的 aa-workflow 声明（代码即 DAG）。
//!
//! 9 节点：asr 提时序 → asr_ocr_pre → asr_ocr → asr_ocr_fix 校正 → translate
//! → split_audio（仅时序）→ mix_video 烧字幕。无 tts / mix_audio。
//! 产物走盘上文件，`deps` 只表达顺序。

use std::sync::Arc;

use aa_workflow_core::{CreateWorkflowConfig, WorkflowDefinition, create_workflow};
use aa_workflow_middlewares::{DagSpec, dag_to_handler, define_dag};
use serde_json::Value;

use crate::input::WorkflowInput;
use crate::steps::asr::args::AsrDeps;
use crate::steps::asr::step_asr;
use crate::steps::asr_ocr::args::AsrOcrDeps;
use crate::steps::asr_ocr::fix_args::AsrOcrFixDeps;
use crate::steps::asr_ocr::pre_args::AsrOcrPreDeps;
use crate::steps::asr_ocr::{fix::step_asr_ocr_fix, ocr::step_asr_ocr, pre::step_asr_ocr_pre};
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
    DagSpec::new("subtitle-asr-ocr")
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
        .node("asr_ocr_pre", ["asr"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_asr_ocr_pre(sc, AsrOcrPreDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("asr_ocr", ["asr_ocr_pre"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_asr_ocr(sc, AsrOcrDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("asr_ocr_fix", ["asr_ocr"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_asr_ocr_fix(sc, AsrOcrFixDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("translate", ["asr_ocr_fix"], |sc, _deps| async move {
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

pub fn subtitle_asr_ocr_spec() -> anyhow::Result<Arc<DagSpec<WorkflowInput, ()>>> {
    define_dag(build_spec())
}

pub fn subtitle_asr_ocr_with_policies() -> anyhow::Result<aa_workflow_core::WorkflowDefinition<WorkflowInput, Value, ()>> {
    use aa_workflow_middlewares::dag_policies;
    use crate::integration::workflow::view_span::view_span_middleware;
    let spec = subtitle_asr_ocr_spec()?;
    let shape = Arc::new(spec.shape().map_err(|e| anyhow::anyhow!("DAG 形状非法: {e}"))?);
    Ok(create_workflow(
        CreateWorkflowConfig::new("subtitle-asr-ocr")
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
    .middleware(view_span_middleware()).middleware(dag_policies(shape))
    .handler(dag_to_handler(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_asr_ocr_spec_is_valid() {
        let spec = subtitle_asr_ocr_spec().expect("静态边表应合法");
        assert_eq!(spec.nodes.len(), 9);
        assert!(!spec.nodes.contains_key("tts"));
        assert_eq!(spec.nodes["asr_ocr_pre"].deps, vec!["asr"]);
        assert_eq!(spec.nodes["translate"].deps, vec!["asr_ocr_fix"]);
    }
}
