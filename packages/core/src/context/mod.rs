use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;
use time::FrameRate;
pub mod types;
pub use types::{StageStatus, WorkflowStage};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VideoSource {
    Youtube,
    Bilibili,
    Local,
    Remote,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct WorkflowBrief {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub current_stage: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Workflow {
    pub id: String,
    pub source: VideoSource,
    pub url: String,
    pub title: Option<String>,
    pub status: String,
    pub current_stage: Option<String>,
    pub workflow_dir: String,
    pub final_video_path: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}
impl From<Workflow> for WorkflowBrief {
    fn from(t: Workflow) -> Self {
        Self {
            id: t.id,
            title: t.title,
            status: t.status,
            current_stage: t.current_stage,
            created_at: t.created_at,
            started_at: t.started_at,
            completed_at: t.completed_at,
            error_message: t.error_message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AsrRunInfo {
    pub engine: String,
    pub device: String,
    pub compute_type: Option<String>,
    pub gpu_attempted: Option<bool>,
    pub fallback_to_cpu: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RunInfo {
    pub asr: Option<AsrRunInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkflowCtx {
    pub workflow: Workflow,
    pub stages: Option<Vec<WorkflowStage>>,
    pub pipeline: String,
    pub last_run_pipeline: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub input: serde_json::Value,
    pub frame_rate: FrameRate,
    pub run_info: Option<RunInfo>,
    pub video_source_path: Option<String>,
    pub audio_source_path: Option<String>,
    pub asr_language: Option<crate::r#const::lang::Language>,
    pub target_language: Option<String>,
}

pub fn ctx_path(workflow_dir: &str) -> PathBuf {
    PathBuf::from(workflow_dir).join("ctx.json")
}

pub fn read_ctx(workflow_dir: &str) -> Result<WorkflowCtx, String> {
    let path = ctx_path(workflow_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    read_ctx_from_value(json)
}

/// 从 ctx.json 的 JSON Value 解析 WorkflowCtx (不读写文件, 供测试/透传直接构造)
pub fn read_ctx_from_value(json: serde_json::Value) -> Result<WorkflowCtx, String> {
    let workflow: Workflow = json
        .get("workflow")
        .ok_or_else(|| "Missing 'workflow' in ctx".to_string())
        .and_then(|v| {
            serde_json::from_value(v.clone()).map_err(|e| format!("Failed to parse workflow: {}", e))
        })?;

    let stages = json.get("stages").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|item| serde_json::from_value(item.clone()).ok())
            .collect()
    });

    Ok(WorkflowCtx {
        workflow,
        stages,
        pipeline: json
            .get("pipeline")
            .and_then(|v| v.as_str())
            .unwrap_or("dub")
            .to_string(),
        last_run_pipeline: json
            .get("last_run_pipeline")
            .and_then(|v| v.as_str())
            .map(String::from),
        input: json
            .get("input")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        run_info: json
            .get("run_info")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        video_source_path: json
            .get("video_source_path")
            .and_then(|v| v.as_str())
            .map(String::from),
        audio_source_path: json
            .get("audio_source_path")
            .and_then(|v| v.as_str())
            .map(String::from),
        asr_language: json
            .get("asr_language")
            .and_then(|v| v.as_str())
            .map(crate::r#const::lang::Language::from),
        target_language: json
            .get("target_language")
            .and_then(|v| v.as_str())
            .map(String::from),
        frame_rate: json
            .get("frame_rate")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(time::FrameRate::FPS_30),
    })
}

/// 把 [`WorkflowCtx`] 序列化写回 `ctx.json` (镜像 TS `writeCtx`)。
pub fn write_ctx(workflow_dir: &str, ctx: &WorkflowCtx) -> Result<(), String> {
    let path = ctx_path(workflow_dir);
    let json = serde_json::to_string_pretty(ctx)
        .map_err(|e| format!("Failed to serialize ctx for {}: {}", workflow_dir, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

pub fn read_workflow(workflow_dir: &str) -> Result<Workflow, String> {
    let path = ctx_path(workflow_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    let _workflow_value = json
        .get("workflow")
        .ok_or_else(|| format!("Missing 'workflow' field in {}", path.display()))?;
    serde_json::from_value(_workflow_value.clone())
        .map_err(|e| format!("Failed to deserialize workflow in {}: {}", path.display(), e))
}

pub fn read_stages(workflow_dir: &str) -> Result<Vec<WorkflowStage>, String> {
    read_ctx(workflow_dir).map(|ctx| ctx.stages.unwrap_or_default())
}

pub fn read_pipeline(workflow_dir: &str) -> String {
    read_ctx(workflow_dir)
        .map(|ctx| ctx.pipeline)
        .unwrap_or_else(|_| "dub".to_string())
}
