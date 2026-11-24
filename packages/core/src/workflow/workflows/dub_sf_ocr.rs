//! dub × sf_ocr 流程的 aa-workflow 声明（代码即 DAG）。
//!
//! 10 步的 `DagSpec` 供引擎按依赖并发地驱动 `ctx.step`——handler 由
//! `dag_to_handler` 生成。
//!
//! 节点闭包不捕获外部变量：从 `sc.ctx.input` 取 [`WorkflowInput`]（typed run
//! input），按 `pipeline.rs::run_step` 的装配方式构造各 `XxxDeps`，调用已有
//! step 函数。跨步骤数据仍 100% 走盘上产物（各 `XxxDeps` 的读集）；DAG 的
//! `deps`（`BTreeMap<String, Value>`）只表达调度顺序并进 checkpoint，节点
//! 一律返回 `Value::Null`——不为 Output 结构体加 `Serialize`，也不把产物
//! 塞进 checkpoint（重放时步骤会重跑，产物从盘上再读）。
//!
//! `TState = ()`：DAG 形态下 state 无写入者，一次 run 内恒定（库文档背书的
//! 「无 state」表达，见 `define_workflow.rs` 的 `CreateWorkflowConfig` 注释）。

use std::sync::Arc;

use aa_workflow_core::{CreateWorkflowConfig, WorkflowDefinition, create_workflow};
use aa_workflow_middlewares::{DagSpec, dag_to_handler, define_dag};
use serde_json::Value;

use crate::input::WorkflowInput;
use crate::steps::mix_audio::args::MixAudioDeps;
use crate::steps::mix_audio::step_mix_audio;
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
use crate::steps::tts::args::TtsDeps;
use crate::steps::tts::step_tts;

/// 10 个节点 + 依赖边。
///
/// ```text
/// separate ─┬─► separate_after ─────────────────────────────────────┐ (bgm)
///           │                                                       ▼
///           └────────────────► split_audio ─► tts ─► mix_audio ─► mix_video
///                          ▲                                    ▲
///   sf_ocr_pre ─► sf_ocr ─┴─► sf_ocr_fix ─► translate ──────────┘
/// ```
///
/// `deps` 参数保留但不读——步骤产物走盘上 `XxxDeps::from_input`，这里只借它
/// 表达「等谁跑完」；`Value::Null` 进 checkpoint 仅作顺序记录。
fn build_spec() -> DagSpec<WorkflowInput, ()> {
    DagSpec::new("dub-sf-ocr")
        // 分支 A：分离人声 + 伴奏。
        .node("separate", Vec::<&str>::new(), |sc, _deps| async move {
            step_separate(sc).await?;
            Ok(Value::Null)
        })
        .node("separate_after", ["separate"], |sc, _deps| async move {
            let wfi = sc.ctx.input.clone();
            step_separate_after(sc, SeparateAfterDeps::from_input(&wfi)).await?;
            Ok(Value::Null)
        })
        // 分支 B：抽关键帧 → OCR → 校正 → 翻译。
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
        // 汇聚链：split_audio 等 separate + translate（与 dag.rs 一致），
        // mix_video 等 mix_audio + separate_after 的 bgm。
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

/// 校验后的 spec（重复 / 未知依赖 / 环在定义期就被 `define_dag` 拦下）。
/// 策略中间件（redo / 目标闭包）也从这同一份图派生形状。
pub fn dub_sf_ocr_spec() -> anyhow::Result<Arc<DagSpec<WorkflowInput, ()>>> {
    define_dag(build_spec())
}

/// 带 DAG 策略中间件的完整定义：redo 复用 + 临时目标泊车。
///
/// `get_events` 供中间件读源 run 的事件日志（reseed 复用未失效步骤的结果）。
/// start 路径也用这个定义，但**不传 DagPlan meta**——中间件 `try_get` 缺失时
/// 直接放行，不做任何策略。
pub fn dub_sf_ocr_with_policies(
    get_events: Arc<aa_workflow_middlewares::GetEvents>,
) -> anyhow::Result<aa_workflow_core::WorkflowDefinition<WorkflowInput, Value, ()>> {
    use aa_workflow_middlewares::dag_policies;

    let spec = dub_sf_ocr_spec()?;
    let shape = Arc::new(spec.shape().map_err(|e| anyhow::anyhow!("DAG 形状非法: {e}"))?);
    Ok(create_workflow(
        CreateWorkflowConfig::new("dub-sf-ocr")
            .input::<WorkflowInput>()
            .state::<()>()
            .initialize(|input: &WorkflowInput| {
                anyhow::ensure!(
                    input
                        .workflow
                        .video_dir
                        .as_deref()
                        .map_or(false, |s| !s.is_empty()),
                    "workflow.video_dir 缺失（start 由 import 回填，continue 必填）"
                );
                Ok(())
            }),
    )
    .middleware(dag_policies(shape, get_events))
    .handler(dag_to_handler(spec)))
}

/// 无中间件的定义（仅用于不需要策略的独立测试/检查）。
pub fn dub_sf_ocr() -> WorkflowDefinition<WorkflowInput, Value, ()> {
    let spec = dub_sf_ocr_spec().expect("dub spec 合法（无重复 / 未知依赖 / 环）");
    create_workflow(
        CreateWorkflowConfig::new("dub-sf-ocr")
            .input::<WorkflowInput>()
            .state::<()>()
            .initialize(|input: &WorkflowInput| {
                anyhow::ensure!(
                    input
                        .workflow
                        .video_dir
                        .as_deref()
                        .map_or(false, |s| !s.is_empty()),
                    "workflow.video_dir 缺失（start 由 import 回填，continue 必填）"
                );
                Ok(())
            }),
    )
    .handler(dag_to_handler(spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dub_spec_is_valid() {
        let spec = dub_sf_ocr_spec().expect("静态边表应合法");
        assert_eq!(spec.nodes.len(), 10);
        assert_eq!(spec.nodes["translate"].deps, vec!["sf_ocr_fix"]);
        assert_eq!(spec.nodes["mix_video"].deps, vec!["mix_audio", "separate_after"]);
    }
}
