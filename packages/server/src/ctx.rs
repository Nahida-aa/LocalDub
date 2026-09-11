use std::path::PathBuf;
use std::sync::Arc;

use axum::http::HeaderMap;

use crate::feat::workflows::queue::WorkflowQueue;

#[derive(Clone)]
pub struct AppState {
    pub repo_root: PathBuf,
    /// 触发主服务器优雅关闭 (`fnrpc shutdown` / 外部调用)。
    pub shutdown: Arc<tokio::sync::Notify>,
    /// 任务队列 (CLI 通过 fnrpc 入队, worker 串行执行)。
    pub queue: Arc<WorkflowQueue>,
}

impl AppState {
    pub fn new() -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // packages/server → packages → repo root (2 级; 原 app/src-tauri 是 3 级)
        let repo_root = dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or(dir);
        Self {
            repo_root,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            queue: WorkflowQueue::new(),
        }
    }
}

pub struct Ctx {
    pub state: AppState,
    pub headers: HeaderMap,
}
