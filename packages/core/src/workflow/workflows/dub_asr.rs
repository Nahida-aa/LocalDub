//! dub × asr 流程的 aa-workflow 声明（代码即 DAG）。
//!
//! 与 [`crate::workflow::dag`] 无关——那是过渡 parallel 调度用的结构；这里是
//! 引擎直接驱动的 `DagSpec`：节点集/依赖边按 `DUB_STEPS` 与各 XxxDeps 读集定义。
//! 节点一律返回 `Value::Null`（产物走盘上文件、`deps` 只表达顺序）。

use std::sync::Arc;

use aa_workflow_core::{CreateWorkflowConfig, WorkflowDefinition, create_workflow};
use aa_workflow_middlewares::{DagSpec, dag_to_handler, define_dag};
use serde_json::Value;

use crate::input::WorkflowInput;
use crate::steps::asr::args::AsrDeps;
use crate::steps::asr::fix_args::AsrFixDeps;
use crate::steps::asr::fix::step_asr_fix;
use crate::steps::asr::step_asr;
use crate::steps::mix_audio::args::MixAudioDeps;
use crate::steps::mix_audio::step_mix_audio;
use crate::steps::mix_video::args::MixVideoDeps;
use crate::steps::mix_video::step_mix_video;
use crate::steps::separate::{
    after_args::SeparateAfterDeps, step_separate, step_separate_after,
};
use crate::steps::split_audio::args::SplitAudioDeps;
use crate::steps::split_audio::step_split_audio;
use crate::steps::translate::args::TranslateDeps;
use crate::steps::translate::step_translate;
use crate::steps::tts::args::TtsDeps;
use crate::steps::tts::step_tts;

/// 9 节点：separate/separate_after + asr/asr_fix 字幕链 + 汇聚链。
fn build_spec() -> DagSpec<WorkflowInput, ()> {
    DagSpec::new("dub-asr")
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
        .node("tts", ["split_audio"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_tts(sc, TtsDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        .node("mix_audio", ["tts"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_mix_audio(sc, MixAudioDeps::from_input(&wfi)?).await?;
            Ok(Value::Null)
        })
        .node(
            "mix_video",
            ["mix_audio", "separate_after"],
            |sc, _deps| async move {
                let wfi = sc.ctx.input.clone();
                step_mix_video(sc, MixVideoDeps::from_input(&wfi)?).await?;
                Ok(Value::Null)
            },
        )
}

pub fn dub_asr_spec() -> anyhow::Result<Arc<DagSpec<WorkflowInput, ()>>> {
    define_dag(build_spec())
}

pub fn dub_asr_with_policies(
    get_events: Arc<aa_workflow_middlewares::GetEvents>,
) -> anyhow::Result<aa_workflow_core::WorkflowDefinition<WorkflowInput, Value, ()>> {
    use aa_workflow_middlewares::dag_policies;
    let spec = dub_asr_spec()?;
    let shape = Arc::new(spec.shape().map_err(|e| anyhow::anyhow!("DAG 形状非法: {e}"))?);
    Ok(create_workflow(
        CreateWorkflowConfig::new("dub-asr")
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
    fn dub_asr_spec_is_valid() {
        let spec = dub_asr_spec().expect("静态边表应合法");
        assert_eq!(spec.nodes.len(), 9);
        assert!(spec.nodes.contains_key("asr"));
        assert!(spec.nodes.contains_key("asr_fix"));
        assert!(spec.nodes.contains_key("tts"));
        // asr 链末端 → translate → split_audio
        assert_eq!(spec.nodes["translate"].deps, vec!["asr_fix"]);
        assert_eq!(spec.nodes["asr_fix"].deps, vec!["asr"]);
    }
}
