//! `command=video_action` 派发器：对某个已导入视频任务的动作（video_dir 必填）。
//!
//! 针对一个已导入目录的**非工作流**动作——status/get_workflow_ctx/generate_meta。
//! 工作流执行（start/resume）归 `command=workflow`（[`crate::cmd::workflows::workflow`]）。
//! `video_dir` 由 [`VideoActionArgs`] 类型层保证必填，不再像旧路径那样运行时检查。
//!
//! 实现策略：`VideoActionArgs` 带的是任务级参数（video_dir + 可选执行参数），
//! 步骤配置（steps / llm_providers / 其它 workflow 参数）仍来自磁盘 input.json
//! 或顶层 Input（CLI 的 input.jsonc）——用基底 Input 注入这些参数后复用既有 handler。

use crate::input::{Input, VideoAction, VideoActionArgs};
use crate::state::{read_input, read_state, read_steps_state};

pub async fn cmd_video_action(args: &VideoActionArgs) -> anyhow::Result<()> {
    match args.action {
        VideoAction::Status => {
            let video_dir = &args.video_dir;
            let wf = read_state(video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/view.json 失败: {e}"))?
                .workflow;
            println!("workflow.status = {}", wf.status);
            for (name, st) in read_steps_state(video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/<step>/state.json 失败: {e}"))?
                .0
            {
                println!("  {}: {:?}", name.as_str(), st.status);
            }
        }
        VideoAction::GetWorkflowCtx => {
            let video_dir = &args.video_dir;
            let st = read_state(video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/view.json 失败: {e}"))?;
            let input = read_input(video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/input.json 失败: {e}"))?;
            let steps = read_steps_state(video_dir)
                .map_err(|e| anyhow::anyhow!("读取 {video_dir}/<step>/state.json 失败: {e}"))?;
            let json = serde_json::to_string_pretty(&serde_json::json!({
                "state": st, "input": input, "steps": steps.0,
            }))
            .map_err(|e| anyhow::anyhow!("序列化 ctx 失败: {e}"))?;
            println!("{json}");
        }
        VideoAction::GenerateMeta => {
            let base = base_input(args)?;
            crate::cmd::workflows::meta::generate_meta(&base, &args.video_dir)
                .await
                .context("generate_meta 失败")?;
        }
    }
    Ok(())
}

/// 由 VideoActionArgs 构造基底 Input：读回磁盘的 input.json（已有的步骤配置/
/// llm_providers），再把 video_dir 与本次显式执行参数注入 workflow。
///
/// 磁盘 input.json 不存在时（理论上不会——video_dir 必填且已导入），
/// 退回顶层默认 Input 并只填 video_dir。
fn base_input(args: &VideoActionArgs) -> anyhow::Result<Input> {
    let mut input = read_input(&args.video_dir).unwrap_or_default();
    let mut workflow = input.workflow.clone().unwrap_or_default();
    workflow.video_dir = Some(args.video_dir.clone());
    // run-kind 标记：步骤（如 tts）据此区分「全量」与「续跑」。
    // Status / GetWorkflowCtx 不走 base_input（直接读文件），这里只需覆盖可执行动作。
    if args.action == VideoAction::GenerateMeta {
        workflow.action = Some(crate::workflow::args::WorkflowAction::Start)
    }
    input.workflow = Some(workflow);
    input.command = crate::input::Command::Workflow;
    Ok(input)
}

use anyhow::Context;
