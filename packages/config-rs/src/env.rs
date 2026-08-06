use crate::root::{config_dir, resolve_path};
use std::path::PathBuf;
use std::sync::OnceLock;

static DOTENV_LOADED: OnceLock<()> = OnceLock::new();

fn ensure_loaded() {
    DOTENV_LOADED.get_or_init(|| {
        let env_path = config_dir().join(".env");
        if env_path.exists() {
            dotenvy::from_path(&env_path).ok();
        }
    });
}

fn var(key: &str) -> Option<String> {
    ensure_loaded();
    std::env::var(key).ok().map(|v| v.trim().to_string())
}

fn var_or(key: &str, fallback: &str) -> String {
    var(key)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn workfolder() -> PathBuf {
    resolve_path(&var_or("WORKFOLDER", "workfolder"))
}

pub fn model_cache_dir() -> PathBuf {
    resolve_path(&var_or("MODEL_CACHE_DIR", "data/models"))
}
