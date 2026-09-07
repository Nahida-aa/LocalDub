//! 任务队列: CLI/桌面 通过 fnrpc 把任务加入队列, 主服务器串行 worker 执行。
//!
//! 持久化 (`<base_dir>/data/queue/`):
//! - `events.ndjson`: 事件日志, **只追加**, 一行一事件
//!   (enqueued/started/done/failed/canceled/requeued)。append 接近原子,
//!   crash 最坏只丢最后一行, 不损历史 (半行在重放时跳过)。
//! - `checkpoint.json`: `{ compact_offset, entries }` — compaction 时的状态
//!   快照 + 行偏移。启动时加载快照 + 重放 [compact_offset, EOF) 得到完整状态;
//!   原子写 (tmp + rename)。
//! - `events.ndjson.old`: compaction 归档的老事件。
//!
//! 事件折叠 (replay) 幂等: 重复事件对同一 id 收敛到同一状态, 因此 compaction
//! 的截断窗口内 crash 最坏产生重复事件, 无害。
//!
//! 单写者: 只有 server 进程写队列 (桌面 UI 的 enqueue 走 HTTP)。
//!
//! worker: 全局串行, 一次跑一个任务, 为批量运行做准备。

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use config_rs::root::base_dir;
use ld_core::input::Input;
use serde::{Deserialize, Serialize};

/// 终态 (done/failed/canceled) 条目保留数, 超出的在 compaction 时归档到 .old
const KEEP_TERMINAL: usize = 500;

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

/// 队列中的一条任务 (对外结构: fnrpc list_queue / CLI)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct QueueEntry {
    pub id: u64,
    /// 完整任务配置 (input.task.action = start/continue/import)
    pub input: Input,
    pub status: QueueStatus,
    pub error: Option<String>,
}

/// 队列事件 (NDJSON 一行一事件)。
///
/// 注意: 只 derive Serialize; **反序列化用手动分派** (`parse_event`)——
/// internally tagged 的 Content 缓冲重放与 Input 树的字段不兼容
/// (实测报 `invalid type: map, expected f64`), 不能用 serde tag 反序列化。
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

/// 手动解析事件行 (绕开 internally tagged 的 Content 缓冲问题)。
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

/// checkpoint 快照 (compaction 时写; 启动时加载后从 compact_offset 行起重放)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Checkpoint {
    #[serde(default)]
    compact_offset: u64,
    #[serde(default)]
    entries: Vec<QueueEntry>,
}

/// 内存中的折叠状态
#[derive(Debug, Default)]
struct QueueState {
    entries: Vec<QueueEntry>,
}

impl QueueState {
    fn entry_mut(&mut self, id: u64) -> Option<&mut QueueEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    fn apply(&mut self, ev: QueueEvent) {
        match ev {
            QueueEvent::Enqueued { input, id, .. } => self.entries.push(QueueEntry {
                id,
                input,
                status: QueueStatus::Queued,
                error: None,
            }),
            QueueEvent::Started { id, .. } => {
                if let Some(e) = self.entry_mut(id) {
                    e.status = QueueStatus::Running;
                }
            }
            QueueEvent::Done { id, .. } => {
                if let Some(e) = self.entry_mut(id) {
                    e.status = QueueStatus::Done;
                    e.error = None;
                }
            }
            QueueEvent::Failed { id, error, .. } => {
                if let Some(e) = self.entry_mut(id) {
                    e.status = QueueStatus::Failed;
                    e.error = Some(error);
                }
            }
            QueueEvent::Canceled { id, .. } => {
                if let Some(e) = self.entry_mut(id) {
                    e.status = QueueStatus::Canceled;
                }
            }
            QueueEvent::Requeued { id, .. } => {
                if let Some(e) = self.entry_mut(id) {
                    e.status = QueueStatus::Queued;
                }
            }
        }
    }

    fn terminal_count(&self) -> usize {
        self.entries.iter().filter(|e| e.status.is_terminal()).count()
    }

    fn next_id(&self) -> u64 {
        self.entries.iter().map(|e| e.id + 1).max().unwrap_or(0)
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
fn old_path() -> PathBuf {
    queue_dir().join("events.ndjson.old")
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
    if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, checkpoint_path()))
    {
        tracing::error!("[queue] 写 checkpoint 失败: {e}");
    }
}

/// 加载状态: checkpoint 快照 + 重放 [compact_offset, EOF) 的事件。
/// 行解析失败 (含 crash 造成的半行) 跳过。
fn load_state() -> QueueState {
    let cp: Checkpoint = std::fs::read_to_string(checkpoint_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut st = QueueState {
        entries: cp.entries,
    };

    let raw = match std::fs::read_to_string(events_path()) {
        Ok(r) => r,
        Err(_) => return st, // 尚无事件日志
    };
    for (n, line) in raw.lines().enumerate() {
        let n = n as u64;
        if n < cp.compact_offset {
            continue; // 已折叠进快照
        }
        match parse_event(line) {
            Some(ev) => st.apply(ev),
            None => tracing::warn!("[queue] 跳过损坏的事件行 {n}"),
        }
    }
    st
}

// ---------------------------------------------------------------------------
// TaskQueue (对外接口不变)
// ---------------------------------------------------------------------------

/// 全局队列状态 (进程内), 持久化见模块文档。
pub struct TaskQueue {
    state: Mutex<QueueState>,
    notify: tokio::sync::Notify,
}

impl TaskQueue {
    pub fn new() -> Arc<Self> {
        let mut state = load_state();

        // 进程重启: 重放出的 Running = 上次进程中断的任务, 重置回 Queued 重新排队
        // (worker 只消费 Queued), 并落 Requeued 事件保持日志完整。
        let interrupted: Vec<u64> = state
            .entries
            .iter()
            .filter(|e| e.status == QueueStatus::Running)
            .map(|e| e.id)
            .collect();
        for id in &interrupted {
            let ts = now_ts();
            append_event(&QueueEvent::Requeued { ts, id: *id });
            state.apply(QueueEvent::Requeued { ts, id: *id });
            tracing::info!("[queue] id={id} 重启恢复, 重新排队");
        }

        Arc::new(Self {
            state: Mutex::new(state),
            notify: tokio::sync::Notify::new(),
        })
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
        let id = {
            let mut st = self.state.lock().unwrap();
            let id = st.next_id();
            st.apply(QueueEvent::Enqueued {
                ts,
                id,
                input: input.clone(),
            });
            id
        };
        append_event(&QueueEvent::Enqueued { ts, id, input });
        tracing::info!("[queue] id={id} enqueued action={action} target={target}");
        self.notify.notify_one();
        id
    }

    /// worker: 取出队首待执行任务 (标记 Running 并返回)。
    fn pop_next(&self) -> Option<QueueEntry> {
        let mut st = self.state.lock().unwrap();
        let entry = st
            .entries
            .iter()
            .find(|e| e.status == QueueStatus::Queued)?
            .clone();
        let ts = now_ts();
        st.apply(QueueEvent::Started { ts, id: entry.id });
        append_event(&QueueEvent::Started { ts, id: entry.id });
        Some(entry)
    }

    /// 标记任务完成/失败。
    fn mark(&self, id: u64, status: QueueStatus, error: Option<String>) {
        let ts = now_ts();
        {
            let mut st = self.state.lock().unwrap();
            match (status, error.clone()) {
                (QueueStatus::Done, _) => st.apply(QueueEvent::Done { ts, id }),
                (QueueStatus::Failed, Some(err)) => {
                    st.apply(QueueEvent::Failed { ts, id, error: err })
                }
                _ => tracing::error!("[queue] mark 非法组合: id={id} status={status:?}"),
            }
        }
        match status {
            QueueStatus::Done => append_event(&QueueEvent::Done { ts, id }),
            QueueStatus::Failed => append_event(&QueueEvent::Failed {
                ts,
                id,
                error: error.unwrap_or_default(),
            }),
            _ => {}
        }
        // 终态超量则 compaction (低频; mark 由单 worker 调用, 无锁竞争问题)
        let terminal = self.state.lock().unwrap().terminal_count();
        if terminal > KEEP_TERMINAL {
            self.compact();
        }
    }

    /// 等待下一个任务 (worker 阻塞)。
    async fn wait_next(&self) {
        self.notify.notified().await;
    }

    /// 当前队列快照 (供 list_queue)。
    pub fn snapshot(&self) -> Vec<QueueEntry> {
        self.state.lock().unwrap().entries.clone()
    }

    /// 取消一个待执行任务。
    pub fn cancel(&self, id: u64) -> bool {
        let ok = {
            let mut st = self.state.lock().unwrap();
            match st.entries.iter().find(|e| e.id == id) {
                Some(e) if e.status == QueueStatus::Queued => {
                    let ts = now_ts();
                    st.apply(QueueEvent::Canceled { ts, id });
                    append_event(&QueueEvent::Canceled { ts, id });
                    true
                }
                _ => false,
            }
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
                self.wait_next().await;
                continue;
            };
            let id = entry.id;
            let input = entry.input;
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

    /// compaction: 终态条目超 KEEP_TERMINAL 时, 把最老的一批事件归档 .old,
    /// 主文件截断到保留段, 并写 checkpoint 快照 (compact_offset 归零 + 全量 entries)。
    ///
    /// 事件折叠幂等, 截断窗口内 crash 最坏产生重复事件, 无害。
    fn compact(&self) {
        let raw = match std::fs::read_to_string(events_path()) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("[queue] compaction 读事件日志失败: {e}");
                return;
            }
        };

        // pass 1: 全量折叠 (checkpoint 之上的增量), 统计终态总数
        let cp: Checkpoint = std::fs::read_to_string(checkpoint_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut folded = QueueState {
            entries: cp.entries.clone(),
        };
        let mut terminal_total = folded.terminal_count();
        for line in raw.lines() {
            if let Some(ev) = parse_event(line) {
                let before = folded.terminal_count();
                folded.apply(ev);
                if folded.terminal_count() > before {
                    terminal_total += 1;
                }
            }
        }
        let archive_target = terminal_total.saturating_sub(KEEP_TERMINAL);
        if archive_target == 0 {
            return;
        }

        // pass 2: 定位保留段起点 = 第 archive_target 个终态事件的行尾
        let mut folded2 = QueueState {
            entries: cp.entries,
        };
        let mut terminal_passed = 0usize;
        let mut byte_off: u64 = 0;
        let mut keep_start: Option<u64> = None;
        let mut total_lines = 0u64;
        for line in raw.lines() {
            let line_bytes = line.len() as u64 + 1;
            total_lines += 1;
            if keep_start.is_none() {
                if let Some(ev) = parse_event(line) {
                    let before = folded2.terminal_count();
                    folded2.apply(ev);
                    if folded2.terminal_count() > before {
                        terminal_passed += 1;
                        if terminal_passed == archive_target {
                            keep_start = Some(byte_off + line_bytes);
                        }
                    }
                }
            }
            byte_off += line_bytes;
        }
        let Some(keep_start) = keep_start else {
            return; // 未找到边界 (理论不发生), 保守不 compact
        };

        // 1) [0, keep_start) 归档 .old (append)
        if let Some(dir) = std::path::Path::new(&old_path()).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut old) = std::fs::OpenOptions::new().create(true).append(true).open(old_path())
        {
            let mut off: u64 = 0;
            for line in raw.lines() {
                let line_bytes = line.len() as u64 + 1;
                if off >= keep_start {
                    break;
                }
                let _ = writeln!(old, "{line}");
                off += line_bytes;
            }
        }
        // 2) 主文件截断到保留段 (保留段是文件尾部的连续段, set_len 即可)
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(events_path()) {
            let _ = f.set_len(keep_start);
        }
        // 3) checkpoint: 截断后主文件从 0 起, 快照 = 全量折叠状态
        let snapshot = folded2.entries.clone();
        write_checkpoint(&Checkpoint {
            compact_offset: 0,
            entries: snapshot.clone(),
        });
        tracing::info!(
            "[queue] compaction: 归档 {archive_target} 个终态 (共 {total_lines} 行), 快照 {} 条",
            snapshot.len()
        );
    }
}

/// 队列任务的日志标识: start 用 url, continue/import 用 taskDir。
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
