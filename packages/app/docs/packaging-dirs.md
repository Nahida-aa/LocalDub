# 打包后数据目录

Tauri v2 已封装跨平台目录映射（`app.data_dir()` / `app.config_dir()` / `app.cache_dir()`）。

## 标准目录

标准目录（来自 XDG Base Dir (https://specifications.freedesktop.org/basedir-spec/latest/) / Apple / Microsoft）：

| 用途            | Linux                         | macOS                                        | Windows                                 | Android                         |
| --------------- | ----------------------------- | -------------------------------------------- | --------------------------------------- | ------------------------------- |
| **持久数据**    | `~/.local/share/aa.localdub/` | `~/Library/Application Support/aa.localdub/` | `%APPDATA%/aa.localdub/` (Roaming)      | `/data/data/aa.localdub/files/` |
| **配置文件**    | `~/.config/aa.localdub/`      | 同 App Support                               | 同 `%APPDATA%/aa.localdub/`             | 同 data dir                     |
| **缓存**        | `~/.cache/aa.localdub/`       | `~/Library/Caches/aa.localdub/`              | `%LOCALAPPDATA%/aa.localdub/cache`      | `/data/data/aa.localdub/cache/` |
| **可执行/包体** | 用户自定                      | `/Applications/LocalDub.app`                 | `%PROGRAMFILES%/LocalDub/` (安装时可选) | APK 包体，不可写                |

## 目录映射规则

- **`workfolder/`** → 持久数据目录（存放所有任务/分组数据）
- **`data/models/`** → 持久数据目录下的 `data/models/`（模型文件可重新下载，也可放缓存目录）
- **`.env` / `input.json`** → 配置目录

## Windows 说明

- `%APPDATA%` → `C:\Users\<用户名>\AppData\Roaming\`（随域漫游，用户安装时不可选）
- `%LOCALAPPDATA%` → `C:\Users\<用户名>\AppData\Local\`（不可漫游，适合缓存）
- `%PROGRAMFILES%` → `C:\Program Files\`（安装时可自定义路径，但运行时有写权限限制）
- Roaming 目录用户不可选安装路径，是 Windows 硬约定
