//! `sf_ocr_dub`——dub × sf_ocr 配音 pipeline（integration 层第一个注册表化的
//! 具名工作流）。
//!
//! authoring 面完全对齐 aa-workflow 示例（examples/store_file/src/
//! dub_sf_ocr.rs）：`create_workflow` + typed `input`/`state` + `initialize`
//! + 显式步骤调用（代码即 DAG，本文件从上到下就是执行序列——不依赖
//! `get_steps`，也不引用 `DUB_SF_OCR_STEPS`；旧路径仍以该常量为序列真源，
//! 两边若漂移，`engine_serial_matches_run_pipeline` 等价性测试会红）。
//!
//! **state 语义（折中：内存镜像）**：`initialize` 每次 start/resume 从
//! state.json 重建内存态；每步取 `ctx.state` 快照喂给 `run_step`（签名已改
//! 吃 `&WorkflowState`，不再自行读盘），步后把**盘上真值拉回 handle**——
//! step 体可能直接写 state.json（translate 的 `persist_target_language` 是
//! 全仓唯一写点）。真相仍是盘上的 state.json（投影架构未越界；Phase 3 的
//! 「state 由日志派生」是后续方向）。
//!
//! `video_dir` 不走参数：引擎把 `videoDir` 注入 run input（`initialize`/
//! handler 从 `ctx.input`/`ctx.state` 取）——对齐例子（videoDir 本就是
//! run input 的一部分）。
//!
//! 驱动仍走 FsRunStore 直驱 core：`--continue-from`/`--target-step` 保留；
//! runtime 层（lease/sweep）等其收益出现再切。

use aa_workflow_core::{
    BaseCtx, StepCtx, WorkflowDefinition, create_workflow, CreateWorkflowConfig,
};

use super::super::engine::drive_step;
use crate::input::Input;
use crate::state::{read_ctx, WorkflowState};
use crate::workflows::args::StepName;

pub const WORKFLOW_ID: &str = "sf_ocr_dub";

/// 构建 `sf_ocr_dub` 定义（id → 构建函数的映射见 [`super::build`]）。
pub fn sf_ocr_dub() -> WorkflowDefinition<Input, serde_json::Value, WorkflowState> {
    create_workflow(
        CreateWorkflowConfig::new(WORKFLOW_ID)
            .input::<Input>()
            .state::<WorkflowState>()
            // state 不落盘：每次 start / resume 从 state.json 重建。
            // videoDir 由引擎注入进 run input（start 路径的 input 原本没有）。
            .initialize(|input: &Input| {
                let dir = input
                    .workflow
                    .as_ref()
                    .and_then(|w| w.video_dir.as_deref())
                    .ok_or_else(|| {
                        anyhow::anyhow!("run input 缺 workflow.videoDir（引擎路径应注入）")
                    })?;
                read_ctx(dir).map_err(anyhow::Error::msg)
            }),
    )
    .handler(|mut ctx: BaseCtx<Input, WorkflowState>| async move {
        let dir = ctx.state.workflow.video_dir.clone();

        ctx.step("separate", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::Separate)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("separate_after", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::SeparateAfter)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("sf_ocr_pre", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::SfOcrPre)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("sf_ocr", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::SfOcr)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("sf_ocr_fix", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::SfOcrFix)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("translate", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::Translate)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("split_audio", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::SplitAudio)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("tts", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::Tts)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("mix_audio", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::MixAudio)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        ctx.step("mix_video", {
            let state = ctx.state.clone();
            move |_sc: StepCtx| drive_step(state, StepName::MixVideo)
        })
        .await?;
        *ctx.state = read_ctx(&dir).map_err(anyhow::Error::msg)?;

        Ok(serde_json::Value::Null)
    })
}
