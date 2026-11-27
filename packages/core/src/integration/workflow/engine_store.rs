//! workflow-core 的文件系统 [`RunStore`] 实现: 引擎持久化落盘到
//! `<base_dir>/<run_id>/`（base_dir 由调用方传入，约定为 `<video_dir>/workflow-store/`）。
//!
//! - `run_state.json` — 运行元数据信封 ([`aa_workflow_core::RunState`])
//! - `logs.jsonl` — append-only 事件日志 (每行一个 JSON [`aa_workflow_core::WorkflowEvent`],
//!   append 走 CAS expected_index, 冲突报 [`StoreError::Conflict`])
//!
//! 一个 [`FsRunStore`] 实例管一个 video_dir 下的**所有 run**，按传入的 run_id 分目录。
//! 并发模型：**单写者 actor 串行化所有读写**（对齐 aa-workflow InMemoryStore 与上游
//! JS 事件循环），不用 `Mutex`——锁会阻塞 tokio worker。actor 持有一个命令队列，
//! 逐条处理 GetState/SetState/Append/GetEvents/...，CAS 检查与追加在 actor 内原子完成。

use std::path::{Path, PathBuf};

use aa_workflow_core::{DeleteReason, RunState, RunStore, StoreError, WorkflowEvent};
use tokio::sync::{mpsc, oneshot};

fn map_io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

type Reply<T> = oneshot::Sender<Result<T, StoreError>>;

enum Cmd {
    GetState {
        run_id: String,
        reply: Reply<Option<RunState>>,
    },
    SetState {
        run_id: String,
        state: RunState,
        reply: Reply<()>,
    },
    DeleteRun {
        run_id: String,
        reply: Reply<()>,
    },
    AppendEvent {
        run_id: String,
        expected_next_index: usize,
        event: WorkflowEvent,
        reply: Reply<()>,
    },
    GetEvents {
        run_id: String,
        reply: Reply<Vec<WorkflowEvent>>,
    },
}

/// `<base_dir>/<run_id>/` 上的 run + events 持久化（单写者 actor，无锁）。
#[derive(Debug, Clone)]
pub struct FsRunStore {
    base_dir: PathBuf,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
}

impl FsRunStore {
    /// `base_dir` 由调用方传入，约定为 `<video_dir>/workflow-store/`。
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        let _ = std::fs::create_dir_all(&base_dir);
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();
        let actor_dir = base_dir.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    Cmd::GetState { run_id, reply } => {
                        let _ = reply.send(read_state(&actor_dir, &run_id));
                    }
                    Cmd::SetState {
                        run_id,
                        state,
                        reply,
                    } => {
                        let _ = reply.send(write_state(&actor_dir, &run_id, &state));
                    }
                    Cmd::DeleteRun { run_id, reply } => {
                        let dir = actor_dir.join(safe_run_id(&run_id));
                        let _ = reply.send(match std::fs::metadata(&dir) {
                            Ok(_) => std::fs::remove_dir_all(&dir).map_err(map_io),
                            Err(_) => Ok(()),
                        });
                    }
                    Cmd::AppendEvent {
                        run_id,
                        expected_next_index,
                        event,
                        reply,
                    } => {
                        let _ = reply.send(append_event(
                            &actor_dir,
                            &run_id,
                            expected_next_index,
                            &event,
                        ));
                    }
                    Cmd::GetEvents { run_id, reply } => {
                        let _ = reply.send(read_events(&actor_dir, &run_id));
                    }
                }
            }
        });
        Self { base_dir, cmd_tx }
    }

}

/// run_id 做目录名时过滤路径分隔符，防目录穿越（run_id 来自外部 input）。
fn safe_run_id(run_id: &str) -> String {
    run_id
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | ' ' => '_',
            _ => c,
        })
        .collect()
}

fn state_path(dir: &Path, run_id: &str) -> PathBuf {
    dir.join(safe_run_id(run_id)).join("run_state.json")
}

fn logs_path(dir: &Path, run_id: &str) -> PathBuf {
    dir.join(safe_run_id(run_id)).join("logs.jsonl")
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(map_io)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(map_io)?;
    std::fs::rename(&tmp, path).map_err(map_io)
}

fn read_state(dir: &Path, run_id: &str) -> Result<Option<RunState>, StoreError> {
    match std::fs::read_to_string(state_path(dir, run_id)) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| StoreError::Io(format!("run_state.json 解析失败: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(map_io(e)),
    }
}

fn write_state(dir: &Path, run_id: &str, state: &RunState) -> Result<(), StoreError> {
    let raw = serde_json::to_string_pretty(state)
        .map_err(|e| StoreError::Io(format!("run_state.json 序列化失败: {e}")))?;
    atomic_write(&state_path(dir, run_id), &raw)
}

fn read_events(dir: &Path, run_id: &str) -> Result<Vec<WorkflowEvent>, StoreError> {
    let raw = match std::fs::read_to_string(logs_path(dir, run_id)) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(map_io(e)),
    };
    let mut events = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        events.push(
            serde_json::from_str(line)
                .map_err(|e| StoreError::Io(format!("logs.jsonl 解析失败: {e}")))?,
        );
    }
    Ok(events)
}

fn append_event(
    dir: &Path,
    run_id: &str,
    expected_next_index: usize,
    event: &WorkflowEvent,
) -> Result<(), StoreError> {
    // 在 actor 内串行执行：read → CAS 校验 → append 原子完成，无需锁。
    let events = read_events(dir, run_id)?;
    let actual = events.len();
    if actual != expected_next_index {
        return Err(StoreError::Conflict {
            run_id: run_id.to_string(),
            expected: expected_next_index,
            actual,
        });
    }
    let line = serde_json::to_string(event)
        .map_err(|e| StoreError::Io(format!("event 序列化失败: {e}")))?;
    let path = logs_path(dir, run_id);
    let mut out = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(map_io(e)),
    };
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push('\n');
    atomic_write(&path, &out)
}

#[async_trait::async_trait]
impl RunStore for FsRunStore {
    async fn get_run_state(&self, run_id: &str) -> Result<Option<RunState>, StoreError> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::GetState {
                run_id: run_id.to_string(),
                reply,
            })
            .map_err(|e| StoreError::Io(e.to_string()))?;
        rx.await.map_err(|e| StoreError::Io(e.to_string()))?
    }

    async fn set_run_state(&self, run_id: &str, state: &RunState) -> Result<(), StoreError> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::SetState {
                run_id: run_id.to_string(),
                state: state.clone(),
                reply,
            })
            .map_err(|e| StoreError::Io(e.to_string()))?;
        rx.await.map_err(|e| StoreError::Io(e.to_string()))?
    }

    async fn delete_run(&self, run_id: &str, _reason: DeleteReason) -> Result<(), StoreError> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::DeleteRun {
                run_id: run_id.to_string(),
                reply,
            })
            .map_err(|e| StoreError::Io(e.to_string()))?;
        rx.await.map_err(|e| StoreError::Io(e.to_string()))?
    }

    async fn append_event(
        &self,
        run_id: &str,
        expected_next_index: usize,
        event: &WorkflowEvent,
    ) -> Result<(), StoreError> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::AppendEvent {
                run_id: run_id.to_string(),
                expected_next_index,
                event: event.clone(),
                reply,
            })
            .map_err(|e| StoreError::Io(e.to_string()))?;
        rx.await.map_err(|e| StoreError::Io(e.to_string()))?
    }

    async fn get_events(&self, run_id: &str) -> Result<Vec<WorkflowEvent>, StoreError> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::GetEvents {
                run_id: run_id.to_string(),
                reply,
            })
            .map_err(|e| StoreError::Io(e.to_string()))?;
        rx.await.map_err(|e| StoreError::Io(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        // 不用系统 /tmp：写进包内 tmp/（gitignored via */tmp/*）。
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn event(step: &str) -> WorkflowEvent {
        WorkflowEvent::StepFinished {
            ts: 1,
            step_id: step.to_string(),
            result: None,
            attempts: vec![],
            meta: None,
        }
    }

    #[tokio::test]
    async fn roundtrip_state_and_events() {
        let dir = temp_dir("rt");
        let store = FsRunStore::new(&dir);
        assert_eq!(store.get_events("r1").await.unwrap().len(), 0);
        assert!(store.get_run_state("r1").await.unwrap().is_none());

        let st = RunState {
            run_id: "r1".into(),
            workflow_id: "w".into(),
            workflow_version: Some("v1".into()),
            status: aa_workflow_core::RunStatus::Finished,
            input: serde_json::json!({"x": 1}),
            output: None,
            error: None,
            awaiting: vec![],
            waiting_for: None,
            pending_approval: None,
            created_at: 1,
            updated_at: 2,
        };
        store.set_run_state("r1", &st).await.unwrap();
        let reloaded = store.get_run_state("r1").await.unwrap().unwrap();
        assert_eq!(reloaded.run_id, "r1");
        assert_eq!(reloaded.status, aa_workflow_core::RunStatus::Finished);

        store
            .append_event("r1", 0, &event("a"))
            .await
            .unwrap();
        store
            .append_event("r1", 1, &event("b"))
            .await
            .unwrap();
        let evs = store.get_events("r1").await.unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[1].step_id(), Some("b"));
    }

    #[tokio::test]
    async fn cas_conflict_and_fresh_reopen() {
        let dir = temp_dir("cas");
        let store = FsRunStore::new(&dir);
        store
            .append_event("r1", 0, &event("a"))
            .await
            .unwrap();
        let err = store
            .append_event("r1", 0, &event("b"))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::Conflict {
                expected: 0,
                actual: 1,
                ..
            }
        ));

        // 新实例重开同一 base_dir 应看到之前的日志
        let reopened = FsRunStore::new(&dir);
        let evs = reopened.get_events("r1").await.unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].step_id(), Some("a"));
    }
}
