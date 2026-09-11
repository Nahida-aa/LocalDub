//! 任务队列: CLI/桌面 通过 fnrpc 把任务加入队列, 主服务器串行 worker 执行。
//!
//! 持久化 (`<base_dir>/data/queue/`), 三层职责分离:
//! - `events.ndjson`: 事件审计日志, **只追加**。职责: 审计 + crash 兜底 ——
//!   checkpoint 落盘前进程死亡时, 重放 `[consumed_offset, EOF)` 补齐状态。
//!   按大小滚动归档 ([`persist::ROTATE_BYTES`])。
//! - `checkpoint.json`: **维护的状态 (读路径载体)** — `{ consumed_offset, active,
//!   terminal }`, 每次状态变化原子重写。启动直接恢复, 正常路径零重放。
//! - 内存: 仅活跃条目 (Queued/Running, 含 Input) + 有界终态缓存 (KEEP_TERMINAL,
//!   轻量展示字段无 Input)。**内存不随历史增长**。
//!
//! 子模块: [`event`] 数据结构与事件解析; [`persist`] 存储 IO。

mod event;
mod persist;

pub use event::{QueueEntry, QueueStatus};

use std::sync::{Arc, Mutex};

use ld_core::input::Input;

use event::{
    entry_action_target, parse_event, Checkpoint, QueueEvent, TerminalEntry, KEEP_TERMINAL,
};
use persist::{append_event, write_checkpoint, ROTATE_BYTES};

/// 事件折叠: 更新内存的活跃区与终态缓存 (重放与运行终态共用)。
fn finish_entry(
    active: &mut Vec<QueueEntry>,
    terminal: &mut Vec<TerminalEntry>,
    id: u64,
    status: QueueStatus,
    error: Option<String>,
    ts: u64,
) {
    if let Some(pos) = active.iter().position(|e| e.id == id) {
        let e = active.remove(pos);
        terminal.insert(
            0,
            TerminalEntry {
                id,
                action: e.action.clone(),
                target: e.target.clone(),
                status,
                error: error.or(e.error),
                ts,
            },
        );
        terminal.truncate(KEEP_TERMINAL);
    }
}

/// 事件折叠 (重放增量用)。
fn fold(ev: QueueEvent, active: &mut Vec<QueueEntry>, terminal: &mut Vec<TerminalEntry>) {
    match ev {
        QueueEvent::Enqueued { input, id, .. } => {
            let (action, target) = entry_action_target(&input);
            active.push(QueueEntry {
                id,
                status: QueueStatus::Queued,
                error: None,
                input: Some(input),
                action,
                target,
            });
        }
        QueueEvent::Started { id, .. } => {
            if let Some(e) = active.iter_mut().find(|e| e.id == id) {
                e.status = QueueStatus::Running;
            }
        }
        QueueEvent::Done { id, ts, .. } => {
            finish_entry(active, terminal, id, QueueStatus::Done, None, ts)
        }
        QueueEvent::Failed { id, error, ts, .. } => {
            finish_entry(active, terminal, id, QueueStatus::Failed, Some(error), ts)
        }
        QueueEvent::Canceled { id, ts, .. } => {
            finish_entry(active, terminal, id, QueueStatus::Canceled, None, ts)
        }
        QueueEvent::Requeued { id, .. } => {
            if let Some(e) = active.iter_mut().find(|e| e.id == id) {
                e.status = QueueStatus::Queued;
            }
        }
    }
}

/// 全局队列状态 (进程内)。
///
/// 内存只持有活跃条目 + 有界终态缓存; 完整历史在事件日志 (审计) 与
/// checkpoint (展示) 中, 不随任务数增长。
pub struct WorkflowQueue {
    /// 活跃条目 (Queued/Running), id 升序
    active: Mutex<Vec<QueueEntry>>,
    /// 最近终态 (新→旧, 有界 KEEP_TERMINAL)
    terminal: Mutex<Vec<TerminalEntry>>,
    /// 事件日志行数 (= checkpoint consumed_offset 的同步镜像)
    next_line: Mutex<u64>,
    notify: tokio::sync::Notify,
}

impl WorkflowQueue {
    pub fn new() -> Arc<Self> {
        let cp: Checkpoint = std::fs::read_to_string(persist::checkpoint_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut active = cp.active;
        let mut terminal = cp.terminal;
        let file_lines = persist::file_line_count();

        // crash 兜底: checkpoint 落盘后又有事件追加 (未及写 checkpoint),
        // 重放增量补齐。
        if file_lines > cp.consumed_offset {
            let raw = std::fs::read_to_string(persist::events_path()).unwrap_or_default();
            let mut replayed = 0u64;
            for (n, line) in raw.lines().enumerate() {
                if (n as u64) < cp.consumed_offset {
                    continue;
                }
                match parse_event(line) {
                    Some(ev) => {
                        fold(ev, &mut active, &mut terminal);
                        replayed += 1;
                    }
                    None => tracing::warn!("[queue] 跳过损坏的事件行 {n}"),
                }
            }
            if replayed > 0 {
                tracing::info!(
                    "[queue] 重放增量 {replayed} 行恢复状态 (checkpoint offset={})",
                    cp.consumed_offset
                );
            }
        }

        // 进程重启: 重放出的 Running = 上次进程中断的任务, 重置回 Queued 重新排队
        // (worker 只消费 Queued), 并落 Requeued 事件保持日志完整。
        let interrupted: Vec<u64> = active
            .iter()
            .filter(|e| e.status == QueueStatus::Running)
            .map(|e| e.id)
            .collect();
        for id in &interrupted {
            let ts = persist::now_ts();
            append_event(&QueueEvent::Requeued { ts, id: *id });
            if let Some(e) = active.iter_mut().find(|e| e.id == *id) {
                e.status = QueueStatus::Queued;
            }
            tracing::info!("[queue] id={id} 重启恢复, 重新排队");
        }

        terminal.truncate(KEEP_TERMINAL);
        write_checkpoint(&Checkpoint {
            consumed_offset: file_lines,
            active: active.clone(),
            terminal: terminal.clone(),
        });

        Arc::new(Self {
            active: Mutex::new(active),
            terminal: Mutex::new(terminal),
            next_line: Mutex::new(file_lines),
            notify: tokio::sync::Notify::new(),
        })
    }

    fn next_id(&self) -> u64 {
        let a = self
            .active
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.id + 1)
            .max()
            .unwrap_or(0);
        let t = self
            .terminal
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.id + 1)
            .max()
            .unwrap_or(0);
        a.max(t)
    }

    fn bump_line(&self) {
        *self.next_line.lock().unwrap() += 1;
    }

    /// 重写 checkpoint (内存状态落盘)。
    fn write_cp(&self) {
        let consumed_offset = *self.next_line.lock().unwrap();
        let active = self.active.lock().unwrap().clone();
        let terminal = self.terminal.lock().unwrap().clone();
        write_checkpoint(&Checkpoint {
            consumed_offset,
            active,
            terminal,
        });
    }

    /// 入队一个任务 (完整 input), 返回队列 ID。入队后唤醒 worker。
    pub fn enqueue(&self, input: Input) -> u64 {
        let (action, target) = entry_action_target(&input);
        let target = target.unwrap_or_else(|| "-".into());
        let action_log = action.clone().unwrap_or_else(|| "?".into());

        let ts = persist::now_ts();
        let id = self.next_id();
        append_event(&QueueEvent::Enqueued {
            ts,
            id,
            input: input.clone(),
        });
        self.active.lock().unwrap().push(QueueEntry {
            id,
            status: QueueStatus::Queued,
            error: None,
            input: Some(input),
            action: action.clone(),
            target: Some(target.clone()),
        });
        self.bump_line();
        self.write_cp();
        tracing::info!("[queue] id={id} enqueued action={action_log} target={target}");
        self.notify.notify_one();
        id
    }

    /// worker: 取出队首待执行任务 (标记 Running 并返回)。
    fn pop_next(&self) -> Option<QueueEntry> {
        let entry = {
            let mut a = self.active.lock().unwrap();
            let e = a.iter_mut().find(|e| e.status == QueueStatus::Queued)?;
            e.status = QueueStatus::Running;
            e.clone()
        };
        let ts = persist::now_ts();
        append_event(&QueueEvent::Started { ts, id: entry.id });
        self.bump_line();
        self.write_cp();
        Some(entry)
    }

    /// 标记任务完成/失败: 从活跃区移出 → 终态展示 + 事件落盘 + checkpoint。
    fn mark(&self, id: u64, status: QueueStatus, error: Option<String>) {
        let ts = persist::now_ts();
        {
            let (mut a, mut t) = (self.active.lock().unwrap(), self.terminal.lock().unwrap());
            finish_entry(&mut a, &mut t, id, status, error.clone(), ts);
        }
        match (status, error.clone()) {
            (QueueStatus::Done, _) => append_event(&QueueEvent::Done { ts, id }),
            (QueueStatus::Failed, Some(err)) => {
                append_event(&QueueEvent::Failed { ts, id, error: err })
            }
            _ => {}
        }
        self.bump_line();
        self.write_cp();
    }

    /// 等待下一个任务 (worker 阻塞)。
    async fn wait_next(&self) {
        self.notify.notified().await;
    }

    /// 当前队列快照 (供 list_queue): 活跃条目在前 (id 升序), 终态按新→旧。
    pub fn snapshot(&self) -> Vec<QueueEntry> {
        let mut out = self.active.lock().unwrap().clone();
        for t in self.terminal.lock().unwrap().iter() {
            out.push(QueueEntry {
                id: t.id,
                status: t.status,
                error: t.error.clone(),
                input: None,
                action: t.action.clone(),
                target: t.target.clone(),
            });
        }
        out
    }

    /// 取消一个待执行任务。
    pub fn cancel(&self, id: u64) -> bool {
        let ts = persist::now_ts();
        let ok = {
            let mut a = self.active.lock().unwrap();
            let Some(pos) = a
                .iter()
                .position(|e| e.id == id && e.status == QueueStatus::Queued)
            else {
                return false;
            };
            let e = a.remove(pos);
            {
                let mut t = self.terminal.lock().unwrap();
                t.insert(
                    0,
                    TerminalEntry {
                        id,
                        action: e.action.clone(),
                        target: e.target.clone(),
                        status: QueueStatus::Canceled,
                        error: None,
                        ts,
                    },
                );
                t.truncate(KEEP_TERMINAL);
            }
            append_event(&QueueEvent::Canceled { ts, id });
            self.bump_line();
            self.write_cp();
            true
        };
        if ok {
            tracing::info!("[queue] id={id} canceled");
        }
        ok
    }

    /// 串行 worker 主循环: 等待新任务 → 逐条执行 (一次一个)。
    /// 由调用方 `tokio::spawn` 常驻运行。
    pub async fn run_worker(&self) {
        loop {
            let Some(entry) = self.pop_next() else {
                self.maybe_rotate();
                self.wait_next().await;
                continue;
            };
            let id = entry.id;
            let input = match entry.input {
                Some(i) => i,
                None => {
                    tracing::error!("[queue] id={id} 活跃条目缺 input, 跳过");
                    self.mark(id, QueueStatus::Failed, Some("活跃条目缺 input".into()));
                    continue;
                }
            };
            let target = entry_target(&input).to_string();

            tracing::info!("[queue] id={id} start target={target}");

            let result = tokio::task::spawn_blocking(move || execute_entry(&input)).await;

            match result {
                Ok(Ok(())) => {
                    tracing::info!("[queue] id={id} done target={target}");
                    // 任务真正的完成点在队列 worker (enqueue 的命令早已返回),
                    // 提示音在这里播而非命令退出时。
                    ld_core::cmd::sound::play_workflow_success_bg();
                    self.mark(id, QueueStatus::Done, None);
                }
                Ok(Err(e)) => {
                    let msg = format!("{e:#}");
                    tracing::error!("[queue] id={id} failed target={target}: {msg}");
                    ld_core::cmd::sound::play_workflow_fail_bg();
                    self.mark(id, QueueStatus::Failed, Some(msg));
                }
                Err(e) => {
                    let msg = format!("任务崩溃: {e}");
                    tracing::error!("[queue] id={id} failed target={target}: {msg}");
                    ld_core::cmd::sound::play_workflow_fail_bg();
                    self.mark(id, QueueStatus::Failed, Some(msg));
                }
            }
        }
    }

    /// 事件日志滚动: 超过阈值且 worker 空闲 (活跃区空) 时归档开新文件。
    /// 空闲时滚动保证活跃任务的事件不跨文件, 重放边界始终干净。
    fn maybe_rotate(&self) {
        let size = std::fs::metadata(persist::events_path())
            .map(|m| m.len())
            .unwrap_or(0);
        if size < ROTATE_BYTES {
            return;
        }
        if !self.active.lock().unwrap().is_empty() {
            return;
        }
        let archived =
            persist::queue_dir().join(format!("events-{}.ndjson.old", persist::now_ts()));
        match std::fs::rename(persist::events_path(), &archived) {
            Ok(_) => {
                *self.next_line.lock().unwrap() = 0;
                self.write_cp();
                tracing::info!(
                    "[queue] 事件日志已滚动: {size} bytes -> {}",
                    archived.display()
                );
            }
            Err(e) => tracing::error!("[queue] 滚动事件日志失败: {e}"),
        }
    }
}

/// 队列任务的日志标识: start 用 url, continue/import 用 workflowDir。
fn entry_target(input: &Input) -> &str {
    let workflow = match input.workflow.as_ref() {
        Some(t) => t,
        None => return "-",
    };
    workflow.url
        .as_deref()
        .or(workflow.workflow_dir.as_deref())
        .unwrap_or("-")
}

/// 执行一条队列任务 (同步, 在 spawn_blocking 里跑 ld_core pipeline)。
///
/// 按 `input.workflow.action` 分派:
/// - `start` → import + 完整 pipeline
/// - `continue` → 续跑已有任务
/// - `import` → 只导入 (不跑 pipeline)
fn execute_entry(input: &Input) -> anyhow::Result<()> {
    use ld_core::workflows::args::WorkflowAction;
    let action = input.workflow.as_ref().and_then(|t| t.action);
    match action {
        Some(WorkflowAction::Start) => ld_core::cmd::workflows::start_workflow(input).map(|_| ()),
        Some(WorkflowAction::Continue) => ld_core::cmd::workflows::continue_workflow(input),
        Some(WorkflowAction::Import) => ld_core::cmd::workflows::import_workflow(input).map(|_| ()),
        other => Err(anyhow::anyhow!(
            "队列任务仅支持 start/continue/import, 收到 {other:?}"
        )),
    }
}
