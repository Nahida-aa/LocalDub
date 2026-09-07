use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

/// app data root dir
pub fn base_dir() -> PathBuf {
    #[cfg(not(debug_assertions))]
    if let Some(d) = dirs::data_dir() {
        return d.join("aa.localdub");
    }
    repo_root()
}

pub fn config_dir() -> PathBuf {
    #[cfg(not(debug_assertions))]
    if let Some(d) = dirs::config_dir() {
        return d.join("aa.localdub");
    }
    repo_root()
}

pub fn resolve_path(val: &str) -> PathBuf {
    let p = Path::new(val);
    if p.is_relative() {
        repo_root().join(p)
    } else {
        p.to_path_buf()
    }
}
