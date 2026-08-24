//! 工作区根目录 (镜像 TS `config/path/paths.WORKFOLDER`)。

use std::path::PathBuf;

/// 任务工作区根目录。
///
/// 与 TS `workfolder()` 一致: 默认 `workfolder` (相对仓库根解析为 `<repo>/workfolder`),
/// 可用环境变量 `WORKFOLDER` 覆盖。直接复用 `env::workfolder` 的实现。
pub fn workfolder() -> PathBuf {
    crate::env::workfolder()
}
