//! workflow-core 的文件系统 [`RunStore`] 实现: 引擎持久化落盘到
//! `<workflow_dir>/workflow-engine/`。
//!
//! - `run.json` — 运行元数据信封 ([`workflow_core::RunState`])
//! - `events.jsonl` — append-only 事件日志 (每行一个 JSON [`workflow_core::RunEvent`],
//!   append 走 CAS expected_index, 冲突报 [`StoreError::Conflict`])
//!
//! 与 ctx.json 的关系: ctx.json 是 UI 读取的投影/缓存; 引擎的"成败真相"在本目录的日志。
//! (Phase 3 再把 ctx.json 改造成由日志派生的确定性投影。)

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use workflow_core::{RunEvent, RunState, RunStore, StoreError};

fn map_io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

/// `<workflow_dir>/workflow-engine/` 上的 run + events 持久化。
#[derive(Debug)]
pub struct FsRunStore {
    dir: PathBuf,
    // 序列化 append (读-改-写); 单进程内引擎只有调度线程写, 此锁是防御性的。
    lock: Mutex<()>,
}

impl FsRunStore {
    pub fn new(workflow_dir: &str) -> Self {
        let dir = Path::new(workflow_dir).join("workflow-engine");
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            lock: Mutex::new(()),
        }
    }

    fn run_state_path(&self) -> PathBuf {
        self.dir.join("run.json")
    }

    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    fn atomic_write(path: &Path, contents: &str) -> Result<(), StoreError> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, contents).map_err(map_io)?;
        fs::rename(&tmp, path).map_err(map_io)?;
        Ok(())
    }

    /// 读 events.jsonl: 返回 (events, 事件数)。文件缺失视为空日志。
    fn read_events(&self) -> Result<(Vec<RunEvent>, usize), StoreError> {
        let raw = match fs::read_to_string(self.events_path()) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(e) => return Err(map_io(e)),
        };
        let mut events = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            events.push(
                serde_json::from_str(line)
                    .map_err(|e| StoreError::Io(format!("events.jsonl 解析失败: {e}")))?,
            );
        }
        let n = events.len();
        Ok((events, n))
    }
}

impl RunStore for FsRunStore {
    fn get_run_state(&self, _run_id: &str) -> Result<Option<RunState>, StoreError> {
        match fs::read_to_string(self.run_state_path()) {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(|e| StoreError::Io(format!("run.json 解析失败: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(map_io(e)),
        }
    }

    fn set_run_state(&self, _run_id: &str, state: &RunState) -> Result<(), StoreError> {
        let raw = serde_json::to_string_pretty(state)
            .map_err(|e| StoreError::Io(format!("run.json 序列化失败: {e}")))?;
        Self::atomic_write(&self.run_state_path(), &raw)
    }

    fn delete_run(&self, _run_id: &str) -> Result<(), StoreError> {
        match self.dir.metadata() {
            Ok(_) => fs::remove_dir_all(&self.dir).map_err(map_io),
            Err(_) => Ok(()),
        }
    }

    fn append_event(
        &self,
        run_id: &str,
        expected_next_index: usize,
        event: &RunEvent,
    ) -> Result<(), StoreError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let (_, actual) = self.read_events()?;
        if actual != expected_next_index {
            return Err(StoreError::Conflict {
                run_id: run_id.to_string(),
                expected: expected_next_index,
                actual,
            });
        }
        let line = serde_json::to_string(event)
            .map_err(|e| StoreError::Io(format!("event 序列化失败: {e}")))?;
        let mut out = match fs::read_to_string(self.events_path()) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(map_io(e)),
        };
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&line);
        out.push('\n');
        Self::atomic_write(&self.events_path(), &out)
    }

    fn get_events(&self, _run_id: &str) -> Result<Vec<RunEvent>, StoreError> {
        self.read_events().map(|(v, _)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!("ld_fsstore_{name}_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn event(run_id: &str, step: &str) -> RunEvent {
        RunEvent::StepFinished {
            ts: 1,
            run_id: run_id.to_string(),
            step_id: step.to_string(),
            result: None,
            attempts: vec![],
        }
    }

    #[test]
    fn roundtrip_state_and_events() {
        let dir = temp_dir("rt");
        let store = FsRunStore::new(&dir);
        assert_eq!(store.get_events("r1").unwrap().len(), 0);
        assert!(store.get_run_state("r1").unwrap().is_none());

        let st = RunState {
            run_id: "r1".into(),
            workflow_id: "w".into(),
            workflow_version: Some("v1".into()),
            status: workflow_core::RunStatus::Finished,
            input: serde_json::json!({"x": 1}),
            output: None,
            error: None,
            created_at: 1,
            updated_at: 2,
        };
        store.set_run_state("r1", &st).unwrap();
        let reloaded = store.get_run_state("r1").unwrap().unwrap();
        assert_eq!(reloaded.run_id, "r1");
        assert_eq!(reloaded.status, workflow_core::RunStatus::Finished);

        store.append_event("r1", 0, &event("r1", "a")).unwrap();
        store.append_event("r1", 1, &event("r1", "b")).unwrap();
        let evs = store.get_events("r1").unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[1].step_id(), Some("b"));
    }

    #[test]
    fn cas_conflict_and_fresh_reopen() {
        let dir = temp_dir("cas");
        let store = FsRunStore::new(&dir);
        store.append_event("r1", 0, &event("r1", "a")).unwrap();
        let err = store.append_event("r1", 0, &event("r1", "b")).unwrap_err();
        assert!(matches!(
            err,
            StoreError::Conflict {
                expected: 0,
                actual: 1,
                ..
            }
        ));

        // 新实例重开同一目录应看到之前的日志
        let reopened = FsRunStore::new(&dir);
        let evs = reopened.get_events("r1").unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].step_id(), Some("a"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
