use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    #[default]
    Pending,
    Running,
    // #[serde(rename = "success")]
    Success,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct WorkflowStep {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub status: StepStatus,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub last_message: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}
