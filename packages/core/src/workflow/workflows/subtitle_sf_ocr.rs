//! subtitle × sf_ocr 流程的 aa-workflow 声明（代码即 DAG）。
//!
//! 8 节点：sf_ocr 抽关键帧 OCR → sf_ocr_fix → translate → split_audio（仅时序）
//! → mix_video 烧字幕。无 tts / mix_audio。产物走盘上文件，`deps` 只表达顺序。

use std::sync::Arc;

use aa_workflow_core::{CreateWorkflowConfig, WorkflowDefinition, create_workflow};
use aa_workflow_middlewares::{DagSpec, dag_to_handler, define_dag};
use serde_json::Value;

use crate::input::WorkflowInput;
use crate::steps::mix_video::args::MixVideoDeps;
use crate::steps::mix_video::step_mix_video;
use crate::steps::separate::{
    after_args::SeparateAfterDeps, step_separate, step_separate_after,
};
use crate::steps::sf_ocr::args::SfOcrDeps;
use crate::steps::sf_ocr::fix_args::SfOcrFixDeps;
use crate::steps::sf_ocr::{step_sf_ocr, step_sf_ocr_fix, step_sf_ocr_pre};
use crate::steps::split_audio::args::SplitAudioDeps;
use crate::steps::split_audio::step_split_audio;
use crate::steps::translate::args::TranslateDeps;
use crate::steps::translate::step_translate;

fn build_spec() -> DagSpec<WorkflowInput, ()> {
    DagSpec::new("subtitle-sf-ocr")
        .node("separate", Vec::<&str>::new(), |sc, _deps| async move {
            step_separate(sc).await?;
            Ok(Value::Null)
        })
        .node("separate_after", ["separate"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_separate_after(sc, SeparateAfterDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("sf_ocr_pre", Vec::<&str>::new(), |sc, _deps| async move {
            step_sf_ocr_pre(sc).await?;
            Ok(Value::Null)
        })
        .node("sf_ocr", ["sf_ocr_pre"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_sf_ocr(sc, SfOcrDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("sf_ocr_fix", ["sf_ocr"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_sf_ocr_fix(sc, SfOcrFixDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("translate", ["sf_ocr_fix"], |sc, _deps| async move {
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

pub fn subtitle_sf_ocr_spec() -> anyhow::Result<Arc<DagSpec<WorkflowInput, ()>>> {
    define_dag(build_spec())
}

pub fn subtitle_sf_ocr_with_policies() -> anyhow::Result<aa_workflow_core::WorkflowDefinition<WorkflowInput, Value, ()>> {
    use aa_workflow_middlewares::dag_policies;
    let spec = subtitle_sf_ocr_spec()?;
    let shape = Arc::new(spec.shape().map_err(|e| anyhow::anyhow!("DAG 形状非法: {e}"))?);
    Ok(create_workflow(
        CreateWorkflowConfig::new("subtitle-sf-ocr")
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
    .middleware(dag_policies(shape))
    .handler(dag_to_handler(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_sf_ocr_spec_is_valid() {
        let spec = subtitle_sf_ocr_spec().expect("静态边表应合法");
        assert_eq!(spec.nodes.len(), 8);
        assert!(!spec.nodes.contains_key("tts"));
        assert_eq!(spec.nodes["translate"].deps, vec!["sf_ocr_fix"]);
        assert_eq!(
            spec.nodes["mix_video"].deps,
            vec!["translate", "split_audio", "separate_after"]
        );
    }
}
