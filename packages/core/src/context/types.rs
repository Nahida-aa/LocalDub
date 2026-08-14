use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    #[default]
    Pending,
    Running,
    // #[serde(rename = "success")]
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TaskStage {
    pub name: String,
    pub label: String,
    pub status: StageStatus,
    pub progress: Option<f64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_message: Option<String>,
    pub error_message: Option<String>,
}
