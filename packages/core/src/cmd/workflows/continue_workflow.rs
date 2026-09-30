//! continue 命令派发器 (镜像 TS `packages/core/cmd/workflows/continueTask.ts` 的 `cmdContinueTask`)。
//!
//! 语义 (对齐 TS):
//! 1. 要求 `input.workflow.videoDir` 指定已有任务目录, 否则报错;
//! 2. `setCtx` — 把当前 `input` 写回 `input.json`
//!    (TS `setCtx(videoDir, { input })`), 这样 `continueFrom` / `regenIndices` 等
//!    以当前 `input.jsonc` 为准, 而非沿用陈旧的落盘配置;
//! 3. 读出合并后的 ctx, 交给 [`crate::workflows::continue_pipeline`] 续跑。
//!
//! `input.workflow.engine = true` 时走引擎路径——**开发中，暂不可用**
//! （aa-workflow 集成暂停，见 `integration/workflow/mod.rs`；Engine 臂现打印
//! 「开发中」并报错）。恢复后：`continueFrom`/`targetStep` 由
//! [`crate::integration::workflow::EngineOptions::from_input`] 承载
//! (引擎语义 = 事件日志截断 + 重放短路), input.json 的 setCtx 合并照常执行。
//!
//! 与 [`crate::cmd::workflows::start`] 的区别: continue 不重新 import, 直接沿用既有任务目录。

use crate::input::Input;
use crate::state::write_input;
use crate::workflows::args::ExecPath;
use crate::workflows::continue_pipeline;
use anyhow::Context;

/// 续跑已有任务 (镜像 TS `cmdContinueTask`): 合并 input → 跑 continue_pipeline。
pub fn continue_workflow(input: &Input) -> anyhow::Result<()> {
    let video_dir = input
        .workflow
        .as_ref()
        .and_then(|t| t.video_dir.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("continue 模式需要 input.workflow.videoDir 指定已有任务目录")
        })?;

    if !std::path::Path::new(&video_dir).join("state.json").exists() {
        return Err(anyhow::anyhow!(
            "continue 模式找不到 {}/state.json, 确认 videoDir 正确",
            video_dir
        ));
    }

    // setCtx: 当前 input 落盘到 input.json (镜像 TS setCtx(videoDir, { input }))。
    // 落盘后不再读回——本次续跑用的就是这份 input。
    write_input(&video_dir, input)
        .map_err(|e| anyhow::anyhow!("写回 {}/input.json 失败: {e}", video_dir))?;

    let continue_from = input
        .workflow
        .as_ref()
        .and_then(|t| t.continue_from.clone());
    let label = continue_from
        .as_ref()
        .map(|cf| format!(" from \"{cf:?}\""))
        .unwrap_or_default();
    println!("[cli] 续跑模式, video_dir = {}{}", video_dir, label);

    let path = match input.workflow.as_ref() {
        Some(w) => w.exec_path()?,
        None => ExecPath::Serial,
    };
    match path {
        ExecPath::Engine => {
            // 引擎路径开发中（aa-workflow 集成暂停，见 integration/workflow/mod.rs）。
            // 原接线（恢复时还原）：
            // println!("[cli] 引擎模式 (aa-workflow, 真源 events.jsonl)");
            // crate::integration::workflow::run_workflow_engine(
            //     &video_dir,
            //     &crate::integration::workflow::EngineOptions::from_input(input),
            // )
            // .context("run_workflow_engine 失败")?;
            println!("[cli] 引擎模式开发中, 暂不可用");
            anyhow::bail!("workflow.engine 引擎路径开发中");
        }
        ExecPath::Parallel => {
            println!("[cli] 并行模式 (实验, DAG 调度, 按步跳过已成功步骤)");
            crate::workflows::parallel::run_pipeline_parallel(&video_dir, input)
                .context("run_pipeline_parallel 失败")?;
        }
        ExecPath::Serial => {
            continue_pipeline(&video_dir, input).context("continue_pipeline 失败")?;
        }
    }
    println!("[cli] 完成: {}", video_dir);
    Ok(())
}

// 引擎集成暂停（见 integration/workflow/mod.rs）：依赖 engine_store/test_support 的接线测试一并注释，恢复时还原。
// mod tests {
//     use super::*;
//     use crate::integration::workflow::engine_store::FsRunStore;
//     use crate::integration::workflow::test_support::{setup_subtitle, temp_dir};
//     use aa_workflow_core::{RunStore, WorkflowEvent};
//
//     /// `engine=true` 的 continue 走引擎路径: events.jsonl 落盘 + 重放短路不重记。
//     /// 旧路径 (`continue_pipeline`) 从不写 workflow-engine/，这条把「接线成功」钉住。
//     #[test]
//     fn continue_with_engine_flag_runs_on_engine() {
//         let dir = temp_dir("continue_engine");
//         setup_subtitle(&dir);
//
//         // steps stub 与 fixture 一致——continue 会整体替换 ctx.input, 不能只传 engine
//         let input: Input = serde_json::from_str(&format!(
//             r#"{{"command":"workflow","workflow":{{"action":"continue","videoDir":"{dir}","engine":true}},
//                 "steps":{{"separate":{{"always":false}},"asr":{{"enabled":false}},"asr_fix":{{"enabled":false}},"translate":{{"enabled":false}},"mix_video":{{"enabled":false}}}}}}"#
//         ))
//         .unwrap();
//
//         continue_workflow(&input).unwrap();
//         let events_path = std::path::Path::new(&dir)
//             .join("workflow-engine")
//             .join("events.jsonl");
//         assert!(
//             events_path.exists(),
//             "引擎路径应落盘 workflow-engine/events.jsonl"
//         );
//
//         // 同 video_dir 再续跑: run_id 确定性派生, 重放短路, StepFinished 不翻倍
//         continue_workflow(&input).unwrap();
//         let store = FsRunStore::new(&dir);
//         let events = store.get_events("t").unwrap();
//         let finished = |step: &str| {
//             events
//                 .iter()
//                 .filter(
//                     |e| matches!(e, WorkflowEvent::StepFinished { step_id, .. } if step_id == step),
//                 )
//                 .count()
//         };
//         assert_eq!(
//             finished("separate_after"),
//             1,
//             "重放后不应重记 separate_after"
//         );
//         assert_eq!(finished("split_audio"), 1, "重放后不应重记 split_audio");
//
//         let wf = crate::state::read_workflow(&dir).unwrap();
//         assert_eq!(wf.status, "success");
//
//         let _ = std::fs::remove_dir_all(&dir);
//     }
// }
//