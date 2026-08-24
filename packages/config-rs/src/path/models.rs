use crate::root::repo_root;
use std::path::{Path, PathBuf};

/// whisper.cpp submodule 根目录
fn whisper_cpp_dir() -> PathBuf {
    repo_root().join("submodule").join("whisper.cpp")
}

/// 定位 whisper.cpp Vulkan 构建产物 `whisper-vulkan` (ggml 运行时)。
///
/// 镜像 TS `whisperVulkanPath`: 依次尝试
/// `build/bin/whisper-vulkan`、`build/Release/whisper-vulkan`、
/// `build/bin/Release/whisper-vulkan`、`build/whisper-vulkan`,
/// 命中即返回; 全部缺失时回退到 `build/bin/whisper-vulkan` (提示构建)。
pub fn whisper_vulkan_path() -> PathBuf {
    let base = whisper_cpp_dir().join("build");
    let candidates = [
        base.join("bin").join("whisper-vulkan"),
        base.join("Release").join("whisper-vulkan"),
        base.join("bin").join("Release").join("whisper-vulkan"),
        base.join("whisper-vulkan"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

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

/// 模型缓存根目录 (镜像 TS `MODEL_CACHE_DIR`: 相对路径相对 repo_root 解析, fallback `data/models`)。
pub fn model_cache_dir() -> PathBuf {
    match std::env::var("MODEL_CACHE_DIR") {
        Ok(v) if !v.is_empty() => {
            let p = Path::new(v.trim());
            if p.is_relative() {
                repo_root().join(p)
            } else {
                p.to_path_buf()
            }
        }
        _ => data_dir().join("models"),
    }
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
    repo_root()
        .join("assets")
        .join("media")
        .join("task_success.wav")
}

/// 任务失败提示音路径 (镜像 TS `task_fail_path`)。
pub fn task_fail_path() -> PathBuf {
    repo_root().join("assets").join("media").join("error.wav")
}
