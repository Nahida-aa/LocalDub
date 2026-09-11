//! 队列数据结构: 对外条目、事件与 checkpoint。
//!
//! 事件日志格式: NDJSON 一行一事件。反序列化用手动分派 [`parse_event`] ——
//! internally tagged 的 Content 缓冲重放与 Input 树不兼容 (实测报
//! `invalid type: map, expected f64`), 不能用 serde tag 反序列化。

use ld_core::input::Input;
use serde::{Deserialize, Serialize};

/// 终态展示条目保留数 (checkpoint + 内存缓存), 超出丢弃最老
/// (完整记录仍在事件日志里)。
pub(super) const KEEP_TERMINAL: usize = 500;

/// 队列任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum QueueStatus {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

impl QueueStatus {
    pub(super) fn is_terminal(self) -> bool {
        matches!(
            self,
            QueueStatus::Done | QueueStatus::Failed | QueueStatus::Canceled
        )
    }
}

/// 队列条目 (fnrpc list_queue 对外)。
///
/// 活跃条目 (Queued/Running) 带 `input` (worker 执行需要);
/// 终态条目从活跃区移出后只留展示字段 (`action`/`target`), `input` 为 None。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct QueueEntry {
    pub id: u64,
    pub status: QueueStatus,
    pub error: Option<String>,
    /// 活跃条目的完整任务配置; 终态条目为 None
    pub input: Option<Input>,
    /// 展示: start/continue/import
    pub action: Option<String>,
    /// 展示: start=url / continue|import=videoDir
    pub target: Option<String>,
}

/// 终态展示条目 (checkpoint terminal 列表元素, 无 Input, 新→旧排序)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TerminalEntry {
    pub(super) id: u64,
    pub(super) action: Option<String>,
    pub(super) target: Option<String>,
    pub(super) status: QueueStatus,
    pub(super) error: Option<String>,
    pub(super) ts: u64,
}

/// checkpoint (状态快照, 原子重写)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    /// 已折叠进 active/terminal 的事件行数 (事件日志从该行起为未消费增量)
    #[serde(default)]
    pub(super) consumed_offset: u64,
    #[serde(default)]
    pub(super) active: Vec<QueueEntry>,
    #[serde(default)]
    pub(super) terminal: Vec<TerminalEntry>,
}

/// 队列事件 (NDJSON 一行一事件)。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum QueueEvent {
    Enqueued { ts: u64, id: u64, input: Input },
    Started { ts: u64, id: u64 },
    Done { ts: u64, id: u64 },
    Failed { ts: u64, id: u64, error: String },
    Canceled { ts: u64, id: u64 },
    Requeued { ts: u64, id: u64 },
}

/// 手动解析事件行 (绕开 internally tagged 的 Content 缓冲问题, 见模块文档)。
pub(super) fn parse_event(line: &str) -> Option<QueueEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ts = v.get("ts")?.as_u64()?;
    let id = v.get("id")?.as_u64()?;
    match v.get("type")?.as_str()? {
        "enqueued" => {
            let input: Input = serde_json::from_value(v.get("input")?.clone()).ok()?;
            Some(QueueEvent::Enqueued { ts, id, input })
        }
        "started" => Some(QueueEvent::Started { ts, id }),
        "done" => Some(QueueEvent::Done { ts, id }),
        "failed" => Some(QueueEvent::Failed {
            ts,
            id,
            error: v.get("error")?.as_str()?.to_string(),
        }),
        "canceled" => Some(QueueEvent::Canceled { ts, id }),
        "requeued" => Some(QueueEvent::Requeued { ts, id }),
        _ => None,
    }
}

/// 队列任务的展示字段: action ("start"/"continue"/"import") 与 target。
pub(super) fn entry_action_target(input: &Input) -> (Option<String>, Option<String>) {
    let workflow = match input.workflow.as_ref() {
        Some(t) => t,
        None => return (None, None),
    };
    let action = workflow
        .action
        .map(|a| match a {
            ld_core::workflows::args::WorkflowAction::Start => "start",
            ld_core::workflows::args::WorkflowAction::Continue => "continue",
            ld_core::workflows::args::WorkflowAction::Import => "import",
            _ => "-",
        })
        .map(|s| s.to_string());
    let target = workflow
        .url
        .as_deref()
        .or(workflow.workflow_dir.as_deref())
        .map(|s| s.to_string());
    (action, target)
}
