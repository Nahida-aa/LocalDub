use crate::root::repo_root;
use std::path::PathBuf;

/// app data 根目录 (镜像 TS `DATA_DIR` = <repo>/data)。
pub fn data_dir() -> PathBuf {
    repo_root().join("data")
}

/// cookies 目录 (镜像 TS `COOKIE_DIR`)。
pub fn cookie_dir() -> PathBuf {
    data_dir().join("cookies")
}

/// YouTube cookie 文件路径 (镜像 TS `YOUTUBE_COOKIE_PATH`)。
pub fn youtube_cookie_path() -> PathBuf {
    cookie_dir().join("youtube.txt")
}

pub fn model_cache_dir() -> PathBuf {
    data_dir().join("models")
}

/// 运行时下载的可执行文件目录 (镜像数据目录 `data/bin`)。
///
/// 与 `model_cache_dir` 并列: 关键帧筛选/OCR 等经 GitHub Release 下载的二进制
/// 落于此, 带版本戳 `.version.json` (见 env ensure)。
pub fn bin_dir() -> PathBuf {
    data_dir().join("bin")
}

pub fn demucs_model_dir() -> PathBuf {
    model_cache_dir().join("demucs")
}

pub fn voxcpm_model_dir() -> PathBuf {
    model_cache_dir().join("voxcpm2")
}

/// whisper 模型目录 (镜像 TS `WHISPER_MODEL_DIR` = `<model_cache_dir>/whisper`)。
pub fn whisper_model_dir() -> PathBuf {
    model_cache_dir().join("whisper")
}

/// whisper.cpp ggml 模型默认路径 (ggml-large-v3-turbo.bin)。
pub fn whisper_model_path() -> PathBuf {
    whisper_model_dir().join("ggml-large-v3-turbo.bin")
}

/// 任务成功提示音路径 (镜像 TS `task_success_path`)。
pub fn task_success_path() -> PathBuf {
    repo_root().join("assets").join("media").join("task_success.wav")
}

/// 命令完成提示音路径 (区别于任务完成: enqueue 提交/servers 管理等
/// 命令结束但任务尚未运行/与任务无关)。
pub fn command_done_path() -> PathBuf {
    repo_root()
        .join("assets")
        .join("media")
        .join("命令完成.wav")
}

/// 任务失败提示音路径 (镜像 TS `task_fail_path`)。
pub fn task_fail_path() -> PathBuf {
    repo_root().join("assets").join("media").join("error.wav")
}
