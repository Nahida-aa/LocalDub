//! 队列存储 IO: 事件日志追加、checkpoint 原子重写、路径与常量。

use std::io::Write;
use std::path::PathBuf;

use config_rs::root::repo_root;

use super::event::{Checkpoint, QueueEvent};

/// 事件日志滚动阈值 (字节): 超过且 worker 空闲时归档开新文件。
pub(super) const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub(super) fn queue_dir() -> PathBuf {
    repo_root().join("data").join("queue")
}
pub(super) fn events_path() -> PathBuf {
    queue_dir().join("events.ndjson")
}
pub(super) fn checkpoint_path() -> PathBuf {
    queue_dir().join("checkpoint.json")
}

pub(super) fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 事件日志当前行数。
pub(super) fn file_line_count() -> u64 {
    std::fs::read_to_string(events_path())
        .map(|r| r.lines().count() as u64)
        .unwrap_or(0)
}

/// 追加一个事件 (单行 NDJSON + flush)。
pub(super) fn append_event(ev: &QueueEvent) {
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
pub(super) fn write_checkpoint(cp: &Checkpoint) {
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
