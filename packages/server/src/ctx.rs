use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};

use axum::http::HeaderMap;

#[derive(Clone)]
pub struct AppState {
    pub repo_root: PathBuf,
    pub torch_proc: Arc<Mutex<Option<Child>>>,
    pub voxcpm_proc: Arc<Mutex<Option<Child>>>,
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
            torch_proc: Arc::new(Mutex::new(None)),
            voxcpm_proc: Arc::new(Mutex::new(None)),
        }
    }
}

pub struct Ctx {
    pub state: AppState,
    pub headers: HeaderMap,
}
