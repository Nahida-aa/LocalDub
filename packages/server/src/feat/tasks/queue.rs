//! 任务队列: CLI 通过 fnrpc 把 start/continue 任务加入队列, 主服务器串行执行。
//!
//! 设计:
//! - 持久化到 `workfolder/queue.json` (主服务器重启后队列恢复)。
//! - 全局串行 worker: 队列非空时 pop_front, spawn_blocking 逐个跑 pipeline,
//!   一次只跑一个任务, 为批量运行做准备。
//! - 入队即返回 (enqueue_* RPC 不阻塞), 状态经 ctx.json + watch_task_tree 反馈。
//! - 队列项直接存完整 `Input` (url / taskDir / continueFrom 本就在 input.task 里),
//!   worker 按 input.task.action 分派 start / continue。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ld_core::input::Input;
use serde::{Deserialize, Serialize};

/// 队列任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum QueueStatus {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

/// 队列中的一条任务
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct QueueEntry {
    pub id: u64,
    /// 完整任务配置 (input.task.action = start / continue, url/taskDir/continueFrom 在内)
    pub input: Input,
    pub status: QueueStatus,
    pub error: Option<String>,
}

/// 队列文件内容
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueueFile {
    pub next_id: u64,
    pub entries: Vec<QueueEntry>,
}

/// 队列文件路径: `<workfolder>/queue.json`
fn queue_path() -> PathBuf {
    config_rs::path::paths::workfolder().join("queue.json")
}

fn read_queue() -> QueueFile {
    match std::fs::read_to_string(queue_path()) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => QueueFile::default(),
    }
}

fn write_queue(q: &QueueFile) {
    if let Ok(json) = serde_json::to_string_pretty(q) {
        let _ = std::fs::write(queue_path(), json);
    }
}

/// 全局队列状态 (进程内), 与持久化 queue.json 同步。
pub struct TaskQueue {
    inner: Mutex<QueueFile>,
    /// 通知 worker 有新任务
    notify: tokio::sync::Notify,
    next_id: AtomicU64,
}

impl TaskQueue {
    pub fn new() -> Arc<Self> {
        let mut file = read_queue();
        // 进程重启: 之前卡在 Running 的任务实际已中断, 重置回 Queued 重新排队,
        // 避免永久卡死 (worker 只消费 Queued)。
        for e in file.entries.iter_mut() {
            if e.status == QueueStatus::Running {
                e.status = QueueStatus::Queued;
            }
        }
        if file.entries.iter().any(|e| e.status == QueueStatus::Queued) {
            write_queue(&file);
        }
        let next = file.next_id;
        Arc::new(Self {
            inner: Mutex::new(file),
            notify: tokio::sync::Notify::new(),
            next_id: AtomicU64::new(next),
        })
    }

    /// 入队一个任务 (完整 input), 返回队列 ID。入队后唤醒 worker。
    pub fn enqueue(&self, input: Input) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut q = self.inner.lock().unwrap();
            q.next_id = id + 1;
            q.entries.push(QueueEntry {
                id,
                input,
                status: QueueStatus::Queued,
                error: None,
            });
            write_queue(&q);
        }
        self.notify.notify_one();
        id
    }

    /// worker: 取出队首待执行任务 (标记 Running 并返回)。
    fn pop_next(&self) -> Option<QueueEntry> {
        let mut q = self.inner.lock().unwrap();
        let idx = q.entries.iter().position(|e| e.status == QueueStatus::Queued)?;
        q.entries[idx].status = QueueStatus::Running;
        write_queue(&q);
        Some(q.entries[idx].clone())
    }

    /// 标记任务完成/失败。
    fn mark(&self, id: u64, status: QueueStatus, error: Option<String>) {
        let mut q = self.inner.lock().unwrap();
        if let Some(e) = q.entries.iter_mut().find(|e| e.id == id) {
            e.status = status;
            e.error = error;
        }
        write_queue(&q);
    }

    /// 等待下一个任务 (worker 阻塞)。
    async fn wait_next(&self) {
        self.notify.notified().await;
    }

    /// 当前队列快照 (供 list_queue)。
    pub fn snapshot(&self) -> Vec<QueueEntry> {
        self.inner.lock().unwrap().entries.clone()
    }

    /// 取消一个待执行任务。
    pub fn cancel(&self, id: u64) -> bool {
        let mut q = self.inner.lock().unwrap();
        if let Some(e) = q.entries.iter_mut().find(|e| e.id == id && e.status == QueueStatus::Queued) {
            e.status = QueueStatus::Canceled;
            write_queue(&q);
            true
        } else {
            false
        }
    }

    /// 串行 worker 主循环: 等待新任务 → 逐条执行 (一次一个)。
    /// 由调用方 `tokio::spawn` 常驻运行。
    pub async fn run_worker(&self) {
        loop {
            let Some(entry) = self.pop_next() else {
                self.wait_next().await;
                continue;
            };
            let id = entry.id;
            let input = entry.input;

            let result =
                tokio::task::spawn_blocking(move || execute_entry(&input)).await;

            match result {
                Ok(Ok(())) => self.mark(id, QueueStatus::Done, None),
                Ok(Err(e)) => self.mark(id, QueueStatus::Failed, Some(format!("{e:#}"))),
                Err(e) => self.mark(id, QueueStatus::Failed, Some(format!("任务崩溃: {e}"))),
            }
        }
    }
}

/// 执行一条队列任务 (同步, 在 spawn_blocking 里跑 ld_core pipeline)。
///
/// 按 `input.task.action` 分派:
/// - `start` → import + 完整 pipeline
/// - `continue` → 续跑已有任务
fn execute_entry(input: &Input) -> anyhow::Result<()> {
    use ld_core::tasks::args::TaskAction;
    let action = input.task.as_ref().and_then(|t| t.action);
    match action {
        Some(TaskAction::Start) => ld_core::cmd::tasks::start_task(input).map(|_| ()),
        Some(TaskAction::Continue) => ld_core::cmd::tasks::continue_task(input),
        other => Err(anyhow::anyhow!(
            "队列任务仅支持 start/continue, 收到 {other:?}"
        )),
    }
}
