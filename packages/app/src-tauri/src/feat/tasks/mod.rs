pub mod log;
pub mod tree;

use config_rs::root::base_dir;
use core_rs::{
    cmd::tasks::get_task::GroupInfo,
    context::{self, TaskCtx},
};
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct BatchFileInfo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct BatchFolderResult {
    pub folder_path: String,
    pub files: Vec<BatchFileInfo>,
}

#[fnrpc::rpc_query]
pub async fn get_group_list() -> Result<Vec<GroupInfo>, String> {
    core_rs::cmd::tasks::get_task::get_group_list()
}

#[fnrpc::rpc_query]
pub async fn get_task_ctx(task_dir: String) -> Result<TaskCtx, String> {
    let path = base_dir().join(&task_dir);
    context::read_ctx(
        &path
            .to_str()
            .ok_or_else(|| format!("Invalid task_dir: {}", task_dir))?,
    )
}

#[fnrpc::rpc_mutate]
pub async fn resume_task(task_dir: String, from_stage: String) -> Result<(), String> {
    let abs_task_dir = base_dir().join(&task_dir);
    let abs_task_dir_str = abs_task_dir
        .to_str()
        .ok_or_else(|| "invalid path".to_string())?;

    let ctx_path = abs_task_dir.join("ctx.json");
    let ctx_raw =
        std::fs::read_to_string(&ctx_path).map_err(|e| format!("read ctx.json failed: {}", e))?;
    let mut ctx: serde_json::Value =
        serde_json::from_str(&ctx_raw).map_err(|e| format!("parse ctx.json failed: {}", e))?;

    ctx["input"]["task"]["taskDir"] = serde_json::Value::String(abs_task_dir_str.to_string());
    ctx["input"]["task"]["action"] = serde_json::Value::String("resume".into());
    ctx["input"]["task"]["resumeFrom"] = serde_json::Value::String(from_stage);
    ctx["input"]["stages"]["asr_ocr"]["runtime"] = serde_json::Value::String("ort-py".into());
    ctx["input"]["stages"]["ocr"]["runtime"] = serde_json::Value::String("ort-py".into());
    ctx["input"]["stages"]["tts"]["runtime"] = serde_json::Value::String("cloud".into());

    let input_path = base_dir().join("packages").join("cli").join("input.json");
    std::fs::write(
        &input_path,
        serde_json::to_string_pretty(&ctx["input"]).unwrap(),
    )
    .map_err(|e| format!("write input.json failed: {}", e))?;

    let output = std::process::Command::new("bun")
        .args(["run", "run-task.ts"])
        .current_dir(base_dir().join("packages/cli"))
        .output()
        .map_err(|e| format!("spawn failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// 打开系统文件夹选择对话框，扫描 MP4 文件并返回列表
#[fnrpc::rpc_query]
pub async fn pick_batch_folder() -> Result<Option<BatchFolderResult>, String> {
    let result = tokio::task::spawn_blocking(|| {
        let folder = rfd::FileDialog::new().pick_folder();
        match folder {
            None => Ok(None),
            Some(path) => {
                let mut files: Vec<BatchFileInfo> = Vec::new();
                let entries = std::fs::read_dir(&path)
                    .map_err(|e| format!("Failed to read folder: {}", e))?;
                for entry in entries {
                    let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
                    let p = entry.path();
                    if p.is_file()
                        && p.extension()
                            .map_or(false, |ext| ext.eq_ignore_ascii_case("mp4"))
                    {
                        files.push(BatchFileInfo {
                            name: p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            path: p.to_string_lossy().to_string(),
                        });
                    }
                }
                files.sort_by(|a, b| a.name.cmp(&b.name));
                Ok(Some(BatchFolderResult {
                    folder_path: path.to_string_lossy().to_string(),
                    files,
                }))
            }
        }
    })
    .await
    .map_err(|e| format!("join error: {}", e))?;

    result
}

/// 为单个视频创建新任务并执行配音 pipeline
#[fnrpc::rpc_mutate]
pub async fn start_new_task(
    video_path: String,
    pipeline: String,
    source_lang: Option<String>,
    target_lang: Option<String>,
) -> Result<(), String> {
    let mut task = serde_json::json!({
        "action": "start",
        "url": video_path,
        "pipeline": pipeline,
        "subtitleSource": "asr_ocr",
    });
    if let Some(lang) = &source_lang {
        task["sourceLang"] = serde_json::Value::String(lang.clone());
    }

    let mut stages = serde_json::json!({
        "separate": { "runtime": "pytorch", "device": "cuda" },
        "asr": {
            "runtime": "ggml",
            "device": "cuda",
            "useSeparated": true,
            "mixMode": "vocals",
            "vad": true,
            "vadModel": "silero-v6"
        },
        "asr_ocr": { "runtime": "ort-py" },
        "ocr": { "runtime": "ort-py" },
        "tts": { "runtime": "cloud" }
    });
    if let Some(lang) = &target_lang {
        stages["translate"] = serde_json::json!({ "targetLang": lang });
    }

    let input = serde_json::json!({
        "command": "task",
        "task": task,
        "stages": stages,
    });

    let input_path = base_dir().join("packages").join("cli").join("input.json");
    std::fs::write(&input_path, serde_json::to_string_pretty(&input).unwrap())
        .map_err(|e| format!("write input.json failed: {}", e))?;

    let output = std::process::Command::new("bun")
        .args(["run", "run-task.ts"])
        .current_dir(base_dir().join("packages/cli"))
        .output()
        .map_err(|e| format!("spawn failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
