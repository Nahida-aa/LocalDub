//! `command=queue` 派发器：主服务器任务队列操作（不针对单个任务）。
//!
//! 目前只有 cancel_queue。入队动作（enqueue_*）仍归 `command=workflow`，
//! 只读的 list_queue 归 `command=query`。

use crate::input::QueueArgs;

pub fn cmd_queue(args: &QueueArgs) -> anyhow::Result<()> {
    use crate::input::QueueAction;
    match args.action {
        QueueAction::CancelQueue => {
            crate::cmd::workflows::enqueue::cancel_queue(args.queue_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }
    Ok(())
}
