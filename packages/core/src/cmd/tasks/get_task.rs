use crate::context::{self, TaskBrief};
use crate::utils::time::system_time_to_iso;
use serde::Serialize;
use specta::Type;
use std::fs;

#[derive(Debug, Clone, Serialize, Type)]
pub struct GroupInfo {
    pub group_id: String,
    pub task_count: u32,
    pub created_at: Option<String>,
    pub tasks: Vec<TaskBrief>,
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

        let mut tasks: Vec<TaskBrief> = Vec::new();

        let task_entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for task_entry in task_entries {
            let task_entry = match task_entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let task_path = task_entry.path();
            if !task_path.is_dir() {
                continue;
            }

            if !task_path.join("ctx.json").exists() {
                continue;
            }

            match context::read_task(task_path.to_str().unwrap_or_default()) {
                Ok(task) => {
                    let brief: TaskBrief = task.into();

                    tasks.push(brief);
                }
                Err(_) => continue,
            }
        }

        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let created_at = fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.created().ok())
            .and_then(system_time_to_iso)
            .or_else(|| tasks.last().map(|t| t.created_at.clone()));

        groups.push(GroupInfo {
            group_id,
            task_count: tasks.len() as u32,
            created_at,
            tasks,
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
