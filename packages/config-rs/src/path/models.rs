use crate::root::repo_root;
use std::path::PathBuf;

pub fn model_cache_dir() -> PathBuf {
    repo_root().join("data").join("models")
}

pub fn demucs_model_dir() -> PathBuf {
    model_cache_dir().join("demucs")
}

pub fn voxcpm_model_dir() -> PathBuf {
    model_cache_dir().join("voxcpm2")
}
