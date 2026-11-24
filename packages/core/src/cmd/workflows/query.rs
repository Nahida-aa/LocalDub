//! 纯查询命令派发（`command=query`）。
//!
//! 只读，不修改任务状态：status / get_workflow_ctx / get_group_list。
//! 从 [`crate::cmd::workflows::workflow`]（执行派发器）里拆出，让执行/查询职责分明。
//!
//! 注意：`generate_meta` 虽曾是查询 action，但它需要完整 `Input`（steps.meta +
//! llm_providers），不是纯读，继续留在 `command=workflow` 下。

use crate::cmd::workflows::get_workflow::get_group_list;
use crate::input::QueryArgs;
use crate::state::{read_input, read_state, read_steps_state};

/// 执行一条查询，结果直接打印到 stdout（与其他 cmd handler 一致）。
pub fn cmd_query(args: &QueryArgs) -> anyhow::Result<()> {
    use crate::input::QueryAction;
    match args.action {
        QueryAction::GetGroupList => {
            let groups = get_group_list().map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&groups)
                .map_err(|e| anyhow::anyhow!("序列化 group_list 失败: {e}"))?;
            println!("{json}");
        }
        QueryAction::GetWorkflowCtx => {
            let video_dir = args
                .video_dir
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("get_workflow_ctx 需要 query.video_dir"))?;
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
        QueryAction::Status => {
            let video_dir = args
                .video_dir
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("status 需要 query.video_dir"))?;
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
    }
    Ok(())
}
