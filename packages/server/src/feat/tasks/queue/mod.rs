//! 任务队列: CLI/桌面 通过 fnrpc 把任务加入队列, 主服务器串行 worker 执行。
//!
//! 持久化 (`<base_dir>/data/queue/`), 三层职责分离:
//! - `events.ndjson`: 事件审计日志, **只追加** (enqueued/started/done/failed/
//!   canceled/requeued)。职责: 审计 + crash 兜底 —— checkpoint 落盘前进程死亡时,
//!   重放 `[consumed_offset, EOF)` 补齐状态。按大小滚动归档。
//! - `checkpoint.json`: **维护的状态 (读路径载体)** — `{ consumed_offset, active,
//!   terminal }`, 每次状态变化原子重写 (tmp + rename)。启动直接恢复, 正常路径零重放。
//! - 内存: 仅活跃条目 (Queued/Running, 含 Input) + 有界终态缓存 (KEEP_TERMINAL,
//!   轻量展示字段无 Input)。**内存不随历史增长**。
//!
//! 事件日志格式: NDJSON 一行一事件。反序列化用手动分派 [`parse_event`] ——
//! internally tagged 的 Content 缓冲重放与 Input 树不兼容 (实测报
//! `invalid type: map, expected f64`), 不能用 serde tag 反序列化。

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use config_rs::root::base_dir;
use ld_core::input::Input;
use serde::{Deserialize, Serialize};

/// 终态展示条目保留数 (checkpoint + 内存缓存), 超出丢弃最老
/// (完整记录仍在事件日志里)。
const KEEP_TERMINAL: usize = 500;
/// 事件日志滚动阈值 (字节): 超过且 worker 空闲时归档开新文件。
const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

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
    fn is_terminal(self) -> bool {
        matches!(self, QueueStatus::Done | QueueStatus::Failed | QueueStatus::Canceled)
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
    /// 展示: start=url / continue|import=taskDir
    pub target: Option<String>,
}

/// 终态展示条目 (checkpoint terminal 列表元素, 无 Input, 新→旧排序)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerminalEntry {
    id: u64,
    action: Option<String>,
    target: Option<String>,
    status: QueueStatus,
    error: Option<String>,
    ts: u64,
}

/// checkpoint (状态快照, 原子重写)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Checkpoint {
    /// 已折叠进 active/terminal 的事件行数 (事件日志从该行起为未消费增量)
    #[serde(default)]
    consumed_offset: u64,
    #[serde(default)]
    active: Vec<QueueEntry>,
    #[serde(default)]
    terminal: Vec<TerminalEntry>,
}

/// 队列事件 (NDJSON 一行一事件)。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum QueueEvent {
    Enqueued { ts: u64, id: u64, input: Input },
    Started { ts: u64, id: u64 },
    Done { ts: u64, id: u64 },
    Failed { ts: u64, id: u64, error: String },
    Canceled { ts: u64, id: u64 },
    Requeued { ts: u64, id: u64 },
}

/// 手动解析事件行 (绕开 internally tagged 的 Content 缓冲问题, 见模块文档)。
fn parse_event(line: &str) -> Option<QueueEvent> {
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

// ---------------------------------------------------------------------------
// 存储路径与 IO
// ---------------------------------------------------------------------------

fn queue_dir() -> PathBuf {
    base_dir().join("data").join("queue")
}
fn events_path() -> PathBuf {
    queue_dir().join("events.ndjson")
}
fn checkpoint_path() -> PathBuf {
    queue_dir().join("checkpoint.json")
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_line_count() -> u64 {
    std::fs::read_to_string(events_path())
        .map(|r| r.lines().count() as u64)
        .unwrap_or(0)
}

/// 追加一个事件 (单行 NDJSON + flush)。
fn append_event(ev: &QueueEvent) {
    if let Err(e) = std::fs::create_dir_all(queue_dir()) {
        tracing::error!("[queue] 创建存储目录失败: {e}");
        return;
    }
    let line = match serde_json::to_string(ev) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("[queue] 序列化事件失败: {e}");
            return;
        }
    };
    let mut f = match std::fs::OpenOptions::new().create(true).append(true).open(events_path()) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("[queue] 打开事件日志失败: {e}");
            return;
        }
    };
    if let Err(e) = writeln!(f, "{line}").and_then(|_| f.flush()) {
        tracing::error!("[queue] 追加事件失败: {e}");
    }
}

/// 原子写 checkpoint (tmp + rename)。
fn write_checkpoint(cp: &Checkpoint) {
    if let Err(e) = std::fs::create_dir_all(queue_dir()) {
        tracing::error!("[queue] 创建存储目录失败: {e}");
        return;
    }
    let tmp = checkpoint_path().with_extension("json.tmp");
    let json = match serde_json::to_string_pretty(cp) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!("[queue] 序列化 checkpoint 失败: {e}");
            return;
        }
    };
    if let Err(e) =
        std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, checkpoint_path()))
    {
        tracing::error!("[queue] 写 checkpoint 失败: {e}");
    }
}

// ---------------------------------------------------------------------------
// 状态折叠 (重放与运行共用)
// ---------------------------------------------------------------------------

/// 队列任务的展示字段: action ("start"/"continue"/"import") 与 target。
fn entry_action_target(input: &Input) -> (Option<String>, Option<String>) {
    let task = match input.task.as_ref() {
        Some(t) => t,
        None => return (None, None),
    };
    let action = task
        .action
        .map(|a| match a {
            ld_core::tasks::args::TaskAction::Start => "start",
            ld_core::tasks::args::TaskAction::Continue => "continue",
            ld_core::tasks::args::TaskAction::Import => "import",
            _ => "-",
        })
        .map(|s| s.to_string());
    let target = task
        .url
        .as_deref()
        .or(task.task_dir.as_deref())
        .map(|s| s.to_string());
    (action, target)
}

/// 终态折叠: 从活跃区移出 id, 生成轻量展示条目头插 terminal。
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

// ---------------------------------------------------------------------------
// TaskQueue
// ---------------------------------------------------------------------------

/// 全局队列状态 (进程内)。
///
/// 内存只持有活跃条目 + 有界终态缓存; 完整历史在事件日志 (审计) 与
/// checkpoint (展示) 中, 不随任务数增长。
pub struct TaskQueue {
    /// 活跃条目 (Queued/Running), id 升序
    active: Mutex<Vec<QueueEntry>>,
    /// 最近终态 (新→旧, 有界 KEEP_TERMINAL)
    terminal: Mutex<Vec<TerminalEntry>>,
    /// 事件日志行数 (= checkpoint consumed_offset 的同步镜像)
    next_line: Mutex<u64>,
    notify: tokio::sync::Notify,
}

impl TaskQueue {
    pub fn new() -> Arc<Self> {
        let cp: Checkpoint = std::fs::read_to_string(checkpoint_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut active = cp.active;
        let mut terminal = cp.terminal;
        let file_lines = file_line_count();

        // crash 兜底: checkpoint 落盘后又有事件追加 (未及写 checkpoint),
        // 重放增量补齐。
        if file_lines > cp.consumed_offset {
            let raw = std::fs::read_to_string(events_path()).unwrap_or_default();
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
            let ts = now_ts();
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
        let target = entry_target(&input).to_string();
        let action = input
            .task
            .as_ref()
            .and_then(|t| t.action)
            .map(|a| format!("{a:?}"))
            .unwrap_or_else(|| "?".into());

        let ts = now_ts();
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
            action: Some(action.clone()),
            target: Some(target.clone()),
        });
        self.bump_line();
        self.write_cp();
        tracing::info!("[queue] id={id} enqueued action={action} target={target}");
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
        let ts = now_ts();
        append_event(&QueueEvent::Started { ts, id: entry.id });
        self.bump_line();
        self.write_cp();
        Some(entry)
    }

    /// 标记任务完成/失败: 从活跃区移出 → 终态展示 + 事件落盘 + checkpoint。
    fn mark(&self, id: u64, status: QueueStatus, error: Option<String>) {
        let ts = now_ts();
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
        let ts = now_ts();
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
                    ld_core::cmd::sound::play_task_success_bg();
                    self.mark(id, QueueStatus::Done, None);
                }
                Ok(Err(e)) => {
                    let msg = format!("{e:#}");
                    tracing::error!("[queue] id={id} failed target={target}: {msg}");
                    ld_core::cmd::sound::play_task_fail_bg();
                    self.mark(id, QueueStatus::Failed, Some(msg));
                }
                Err(e) => {
                    let msg = format!("任务崩溃: {e}");
                    tracing::error!("[queue] id={id} failed target={target}: {msg}");
                    ld_core::cmd::sound::play_task_fail_bg();
                    self.mark(id, QueueStatus::Failed, Some(msg));
                }
            }
        }
    }

    /// 事件日志滚动: 超过阈值且 worker 空闲 (活跃区空) 时归档开新文件。
    /// 空闲时滚动保证活跃任务的事件不跨文件, 重放边界始终干净。
    fn maybe_rotate(&self) {
        let size = std::fs::metadata(events_path()).map(|m| m.len()).unwrap_or(0);
        if size < ROTATE_BYTES {
            return;
        }
        if !self.active.lock().unwrap().is_empty() {
            return;
        }
        let archived = queue_dir().join(format!("events-{}.ndjson.old", now_ts()));
        match std::fs::rename(events_path(), &archived) {
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

/// 队列任务的展示标识: start 用 url, continue/import 用 taskDir。
fn entry_target(input: &Input) -> &str {
    let task = match input.task.as_ref() {
        Some(t) => t,
        None => return "-",
    };
    task.url
        .as_deref()
        .or(task.task_dir.as_deref())
        .unwrap_or("-")
}

/// 执行一条队列任务 (同步, 在 spawn_blocking 里跑 ld_core pipeline)。
///
/// 按 `input.task.action` 分派:
/// - `start` → import + 完整 pipeline
/// - `continue` → 续跑已有任务
/// - `import` → 只导入 (不跑 pipeline)
fn execute_entry(input: &Input) -> anyhow::Result<()> {
    use ld_core::tasks::args::TaskAction;
    let action = input.task.as_ref().and_then(|t| t.action);
    match action {
        Some(TaskAction::Start) => ld_core::cmd::tasks::start_task(input).map(|_| ()),
        Some(TaskAction::Continue) => ld_core::cmd::tasks::continue_task(input),
        Some(TaskAction::Import) => ld_core::cmd::tasks::import_task(input).map(|_| ()),
        other => Err(anyhow::anyhow!(
            "队列任务仅支持 start/continue/import, 收到 {other:?}"
        )),
    }
}
