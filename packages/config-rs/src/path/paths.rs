//! 工作区根目录 (镜像 TS `config/path/paths.WORKFOLDER`)。

use crate::root::repo_root;
use std::path::PathBuf;

/// 任务工作区根目录。
///
/// - debug 构建: 仓库根 (`repo_root()`), 与 TS dev 下 `WORKFOLDER` = 仓库根一致
/// - release 构建: `<data_dir>/aa.localdub` (见 `root::base_dir`)
pub fn workfolder() -> PathBuf {
    #[cfg(not(debug_assertions))]
    {
        crate::root::base_dir()
    }
    #[cfg(debug_assertions)]
    {
        repo_root()
    }
}
