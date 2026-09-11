use crate::context::{self, WorkflowBrief};
use crate::utils::time::system_time_to_iso;
use serde::Serialize;
use specta::Type;
use std::fs;

#[derive(Debug, Clone, Serialize, Type)]
pub struct GroupInfo {
    pub group_id: String,
    pub workflow_count: u32,
    pub created_at: Option<String>,
    pub workflows: Vec<WorkflowBrief>,
}

pub fn get_group_list() -> Result<Vec<GroupInfo>, String> {
    let wf = config_rs::env::workfolder();
    let mut groups: Vec<GroupInfo> = Vec::new();

    let entries =
        fs::read_dir(&wf).map_err(|e| format!("Failed to read workfolder {:?}: {}", wf, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let group_id = entry
            .file_name()
            .to_str()
            .ok_or_else(|| format!("Invalid group name: {:?}", path))?
            .to_string();

        let mut workflows: Vec<WorkflowBrief> = Vec::new();

        let _workflow_entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for _workflow_entry in _workflow_entries {
            let _workflow_entry = match _workflow_entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let _workflow_path = _workflow_entry.path();
            if !_workflow_path.is_dir() {
                continue;
            }

            if !_workflow_path.join("ctx.json").exists() {
                continue;
            }

            match context::read_workflow(_workflow_path.to_str().unwrap_or_default()) {
                Ok(workflow) => {
                    let brief: WorkflowBrief = workflow.into();

                    workflows.push(brief);
                }
                Err(_) => continue,
            }
        }

        workflows.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let created_at = fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.created().ok())
            .and_then(system_time_to_iso)
            .or_else(|| workflows.last().map(|t| t.created_at.clone()));

        groups.push(GroupInfo {
            group_id,
            workflow_count: workflows.len() as u32,
            created_at,
            workflows,
        });
    }

    groups.sort_by(|a, b| match (&a.created_at, &b.created_at) {
        (Some(a), Some(b)) => b.cmp(a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.group_id.cmp(&b.group_id),
    });

    Ok(groups)
}
