use std::fs;
use std::path::PathBuf;

use crate::ctx::AppState;

pub fn start_voxcpm(state: &AppState) -> Result<u16, String> {
    // VoxCPM torch server 已迁至 vox-lab (packages/voxcpm_torch_server), LocalDub 暂用云端 TTS。
    let _ = state;
    Err("VoxCPM torch server 正在迁移中, 暂不支持本地启动 (请使用云端 TTS)".into())
}

pub fn stop_voxcpm(state: &AppState) -> Result<(), String> {
    // 随 start_voxcpm 迁移: 本地 voxcpm_proc 恒为空, 仅保留幂等停止语义。
    let _ = state;
    Ok(())
}

fn input_json_path(state: &AppState) -> PathBuf {
    // 与实际 CLI 一致: 写死仓库根 input.jsonc。
    state.repo_root.join("input.jsonc")
}

fn input_schema_path(state: &AppState) -> PathBuf {
    // 跟随 input.jsonc: 写死仓库根 input.schema.json。
    state.repo_root.join("input.schema.json")
}

pub fn read_input(state: &AppState) -> Result<String, String> {
    let path = input_json_path(state);
    fs::read_to_string(&path).map_err(|e| format!("Failed to read input.jsonc: {}", e))
}

pub fn write_input(state: &AppState, content: String) -> Result<(), String> {
    let path = input_json_path(state);
    fs::write(&path, &content).map_err(|e| format!("Failed to write input.jsonc: {}", e))
}

pub fn read_input_schema(state: &AppState) -> Result<String, String> {
    let path = input_schema_path(state);
    fs::read_to_string(&path).map_err(|e| format!("Failed to read input.schema.json: {}", e))
}
