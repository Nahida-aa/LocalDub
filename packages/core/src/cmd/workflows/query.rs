//! 纯查询命令派发（`command=query`）：不针对单个任务——get_group_list / list_queue。
//!
//! 针对某个任务的查询（status / get_workflow_ctx）已归到
//! [`crate::cmd::workflows::video_action`]（video_dir 必填）。

use crate::cmd::workflows::enqueue::list_queue;
use crate::cmd::workflows::get_workflow::get_group_list;
use crate::input::QueryArgs;

pub fn cmd_query(args: &QueryArgs) -> anyhow::Result<()> {
    use crate::input::QueryAction;
    match args.action {
        QueryAction::GetGroupList => {
            let groups = get_group_list().map_err(|e| anyhow::anyhow!("{e}"))?;
            let json = serde_json::to_string_pretty(&groups)
                .map_err(|e| anyhow::anyhow!("序列化 group_list 失败: {e}"))?;
            println!("{json}");
        }
        QueryAction::ListQueue => {
            list_queue().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }
    Ok(())
}
