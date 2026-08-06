pub mod log;
pub mod tree;

use config_rs::{
    root::base_dir,
    // servers::ServerType
};

use core_rs::{
    cmd::tasks::get_task::GroupInfo,
    context::{
        self,
        TaskCtx,
        // Task
    },
};

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

    // 读原始的 ctx.json
    let ctx_path = abs_task_dir.join("ctx.json");
    let ctx_raw =
        std::fs::read_to_string(&ctx_path).map_err(|e| format!("read ctx.json failed: {}", e))?;
    let mut ctx: serde_json::Value =
        serde_json::from_str(&ctx_raw).map_err(|e| format!("parse ctx.json failed: {}", e))?;

    // 只修改 action 和 resumeFrom
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
