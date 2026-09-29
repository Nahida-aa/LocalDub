//! `sf_ocr_dub`——dub × sf_ocr 配音 pipeline（integration 层第一个注册表化的
//! 具名工作流）。
//!
//! authoring 面完全对齐 aa-workflow 示例（examples/store_file/src/
//! dub_sf_ocr.rs）：`create_workflow` + typed `input`/`state` + `initialize`
//! + 显式步骤调用。**两分支并行已启用**（`tokio::try_join!`）——这是
//! LocalDub 现役串行 for 循环做不到的；并行的已知毛边（state.json 投影
//! 并发写互踩、事件日志 CAS 冲突吸收）由真实运行暴露，不在定义层预遮蔽。
//!
//! ## 形状（代码即 DAG，本文件从上到下就是依赖图）
//!
//! ```text
//!              ┌─ separate ──► separate_after ─────────────────► (bgm)
//! import ──────┤                                                   │
//!              └─ sf_ocr_pre ► sf_ocr ► sf_ocr_fix ► translate ─┐  │
//!                                                               ▼  ▼
//!                                split_audio ◄──────────────────┘  │
//!                                    │                             │
//!                                tts ► mix_audio ► mix_video ◄─────┘
//! ```
//!
//! 不依赖 `get_steps`，也不引用 `DUB_SF_OCR_STEPS`（旧路径仍以该常量为
//! 序列真源，两边若漂移，等价性/行为差异由真跑暴露）。
//!
//! **state 语义**：`initialize` 每次 start/resume 从 state.json 重建
//! `ctx.state`（引擎 replay 语义，typed）。步骤的真值读取走盘——
//! `drive_step` 在 step 起点读盘，永远新鲜；分支各持 `ctx.clone()`（state
//! 快照分裂），但步骤只读 state 内容、真源在盘，无碍。真相仍是盘上的
//! state.json（Phase 3 的「state 由日志派生」是后续方向）。
//!
//! `video_dir` 不走参数：引擎把 `videoDir` 注入 run input（`initialize`
//! 从 `ctx.input` 取），`ctx.state` 重建后 handler 从它取一次任务目录。
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
    .handler(|ctx: BaseCtx<Input, WorkflowState>| async move {
        let dir = ctx.state.workflow.video_dir.clone();

        // 分支 A（音频侧）：separate → separate_after
        let branch_audio = {
            let ctx = ctx.clone();
            let dir = dir.clone();
            async move {
                ctx.step("separate", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::Separate)
                })
                .await?;
                ctx.step("separate_after", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::SeparateAfter)
                })
                .await?;
                anyhow::Ok(())
            }
        };

        // 分支 B（字幕侧）：sf_ocr_pre → sf_ocr → sf_ocr_fix → translate
        let branch_subtitle = {
            let ctx = ctx.clone();
            let dir = dir.clone();
            async move {
                ctx.step("sf_ocr_pre", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::SfOcrPre)
                })
                .await?;
                ctx.step("sf_ocr", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::SfOcr)
                })
                .await?;
                ctx.step("sf_ocr_fix", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::SfOcrFix)
                })
                .await?;
                ctx.step("translate", {
                    let dir = dir.clone();
                    move |_sc: StepCtx| drive_step(dir, StepName::Translate)
                })
                .await?;
                anyhow::Ok(())
            }
        };

        // 两分支并行：并发 append 事件走 CAS；state.json 投影写无锁——
        // 丢更新只影响 UI 状态显示，不影响事件日志真源（已知毛边，让问题
        // 显形而不是预遮蔽）。
        tokio::try_join!(branch_audio, branch_subtitle)?;

        // join 之后串行尾段
        ctx.step("split_audio", {
            let dir = dir.clone();
            move |_sc: StepCtx| drive_step(dir, StepName::SplitAudio)
        })
        .await?;
        ctx.step("tts", {
            let dir = dir.clone();
            move |_sc: StepCtx| drive_step(dir, StepName::Tts)
        })
        .await?;
        ctx.step("mix_audio", {
            let dir = dir.clone();
            move |_sc: StepCtx| drive_step(dir, StepName::MixAudio)
        })
        .await?;
        ctx.step("mix_video", {
            let dir = dir.clone();
            move |_sc: StepCtx| drive_step(dir, StepName::MixVideo)
        })
        .await?;
        Ok(serde_json::Value::Null)
    })
}
