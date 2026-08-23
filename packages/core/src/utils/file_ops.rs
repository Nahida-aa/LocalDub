use std::{
    fs,
    path::{Path, PathBuf},
};

use config_rs::root::base_dir;

pub fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))
    } else {
        Ok(())
    }
}

pub fn sanitize_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let cleaned = relative_path.replace('\\', "/");
    if cleaned.contains("..") {
        return Err("Path traversal detected".to_string());
    }
    Ok(base_dir().join(cleaned))
}
