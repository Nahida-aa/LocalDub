use std::{
    fs,
    path::{Path, PathBuf},
};

use config_rs::root::repo_root;
use serde::Serialize;

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
    Ok(repo_root().join(cleaned))
}

/// 把 `value` 序列化为 pretty JSON 并写入 `path`，失败信息带路径。
///
/// 替代各 step 里反复出现的 `to_string_pretty` + `fs::write` + 两段 `map_err` 样板。
/// 返回 `anyhow::Result`，在 step 函数里可直接 `?`。
pub fn write_json_pretty<P: AsRef<Path>, T: Serialize>(
    path: P,
    value: &T,
) -> anyhow::Result<()> {
    let path_disp = path.as_ref().display().to_string();
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| anyhow::anyhow!("序列化 {} 失败: {e}", path_disp))?;
    fs::write(path, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", path_disp, e))?;
    Ok(())
}
