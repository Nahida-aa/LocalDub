//! stage 共享基础设施 (镜像 TS `packages/core/stages/utils/utils.ts` + `context.ts` 的 stage/task 持久化)。
//!
//! 提供:
//! - [`now_iso`] — RFC3339 无毫秒时间戳
//! - [`set_stage`] / [`set_task`] — 对 `ctx.json` 中 stages / task 的 upsert 合并
//! - 各 stage 输出目录 helper ([`separate_dir`] / [`asr_dir`] / [`separate_after_dir`] ...)
//! - [`emit_log`] — tracing + 追加 `<tid>.log`

use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::Serialize;

use crate::context::{TaskStage, read_ctx, write_ctx};

/// RFC3339 时间戳, 去毫秒 (镜像 TS `nowISO`, 形如 `2024-01-01T00:00:00Z`)。
pub fn now_iso() -> String {
    // chrono 默认输出含毫秒 (`.%3f`), 这里截断到秒并补 `Z`。
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 取 task_dir 的最后一段作为 task id (镜像 TS `getLastSegment` / `getTaskId`)。
pub fn task_id(task_dir: &str) -> Option<String> {
    Path::new(task_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// stage / task 持久化 (镜像 TS context.ts 的 setStage / setTask)
// ---------------------------------------------------------------------------

/// 部分更新 [`TaskStage`] 的字段 (对应 TS `Partial<TaskStage>`)。
#[derive(Default, Clone)]
pub struct StagePatch {
    pub label: Option<String>,
    pub status: Option<crate::context::StageStatus>,
    pub progress: Option<f64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_message: Option<String>,
    pub error_message: Option<String>,
}

impl StagePatch {
    /// 合并进已有的 [`TaskStage`] (TS `{...existing, ...patch}`)。
    fn apply(self, mut base: TaskStage) -> TaskStage {
        if let Some(v) = self.label {
            base.label = v;
        }
        if let Some(v) = self.status {
            base.status = v;
        }
        if let Some(v) = self.progress {
            base.progress = Some(v);
        }
        if let Some(v) = self.started_at {
            base.started_at = Some(v);
        }
        if let Some(v) = self.completed_at {
            base.completed_at = Some(v);
        }
        if let Some(v) = self.last_message {
            base.last_message = Some(v);
        }
        if let Some(v) = self.error_message {
            base.error_message = Some(v);
        }
        // 标记 success 时清空 error_message (镜像 TS)
        if matches!(base.status, crate::context::StageStatus::Success) {
            base.error_message = None;
        }
        base
    }
}

/// 对 `ctx.json` 中指定 stage 做 upsert 合并 (镜像 TS `setStage`)。
pub fn set_stage(task_dir: &str, name: &str, patch: StagePatch) -> Result<(), String> {
    let mut ctx = read_ctx(task_dir)?;
    let stages = ctx.stages.get_or_insert_with(Vec::new);
    let idx = stages.iter().position(|s| s.name == name);
    let base = match idx {
        Some(i) => stages[i].clone(),
        None => TaskStage {
            name: name.to_string(),
            label: name.to_string(),
            ..Default::default()
        },
    };
    let updated = patch.apply(base);
    match idx {
        Some(i) => stages[i] = updated,
        None => stages.push(updated),
    }
    write_ctx(task_dir, &ctx)
}

/// 部分更新 [`crate::context::Task`] 的字段 (镜像 TS `setTask`)。
#[derive(Default, Clone)]
pub struct TaskPatch {
    pub status: Option<String>,
    pub current_stage: Option<Option<String>>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub final_video_path: Option<Option<String>>,
}

impl TaskPatch {
    fn apply(self, mut t: crate::context::Task) -> crate::context::Task {
        if let Some(v) = self.status {
            t.status = v;
        }
        if let Some(v) = self.current_stage {
            t.current_stage = v;
        }
        if let Some(v) = self.started_at {
            t.started_at = Some(v);
        }
        if let Some(v) = self.completed_at {
            t.completed_at = Some(v);
        }
        if let Some(v) = self.error_message {
            t.error_message = Some(v);
        }
        if let Some(v) = self.final_video_path {
            t.final_video_path = v;
        }
        // 标记 success 时清空 error_message (镜像 TS)
        if t.status == "success" {
            t.error_message = None;
        }
        t
    }
}

/// 对 `ctx.json` 中 task 做 upsert 合并 (镜像 TS `setTask`)。
pub fn set_task(task_dir: &str, patch: TaskPatch) -> Result<(), String> {
    let mut ctx = read_ctx(task_dir)?;
    ctx.task = patch.apply(ctx.task);
    write_ctx(task_dir, &ctx)
}

// ---------------------------------------------------------------------------
// 输出目录 / 路径 helper (镜像 TS utils.ts 的 *_dir / *_path)
// ---------------------------------------------------------------------------

pub fn separate_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("separate")
}
pub fn asr_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("asr")
}
pub fn separate_after_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("separate_after")
}

/// separate 阶段人声 stem (target_3_vocals.wav)
pub fn vocals_path(task_dir: &str) -> PathBuf {
    separate_dir(task_dir).join("target_3_vocals.wav")
}
/// separate_after 阶段背景音乐 stem
pub fn bgm_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_bgm.wav")
}
/// separate_after 阶段混音后的人声
pub fn mixed_vocals_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_3_vocals_mixed.wav")
}
/// separate_after 阶段 gate 后的人声
pub fn gated_vocals_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_3_vocals_gated.wav")
}

// ---------------------------------------------------------------------------
// 日志 (镜像 TS `emitLog`)
// ---------------------------------------------------------------------------

/// 写日志: tracing info + 追加到 `<task_dir>/<tid>.log`。
pub fn emit_log(task_dir: Option<&str>, line: &str) {
    tracing::info!("{line}");
    let Some(dir) = task_dir else { return };
    let Some(tid) = task_id(dir) else { return };
    let log_path = Path::new(dir).join(format!("{tid}.log"));
    let entry = format!("[{}] {}\n", now_iso(), line);
    // 追加失败不应中断流程
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = f.write_all(entry.as_bytes());
    }
}

/// 序列化辅助: 把任意可序列化值转成 pretty JSON 字符串 (测试 / 透传用)。
pub fn to_json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| e.to_string())
}

/// re-export 以便 stage 模块直接 `use crate::stages::utils::*;`
pub use crate::context::StageStatus;

#[allow(unused_imports)]
use std::io::Write as _;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{TaskCtx, read_ctx_from_value};
    use serde_json::json;

    fn ctx_at(task_dir: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = task_dir.to_string();
        ctx
    }

    #[test]
    fn now_iso_format() {
        let s = now_iso();
        assert!(s.ends_with('Z'));
        assert!(!s.contains('.'), "不应含毫秒: {s}");
        assert_eq!(s.len(), 20);
    }

    #[test]
    fn set_stage_upserts_and_merges() {
        let dir = std::env::temp_dir()
            .join(format!("ld_stage_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "pipeline": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        // 第一次 set → 新建, 带 running + started_at
        set_stage(
            &dir,
            "separate",
            StagePatch {
                status: Some(StageStatus::Running),
                started_at: Some("2024-01-01T00:00:01Z".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // 第二次 set → 合并 progress, 保留 started_at
        set_stage(
            &dir,
            "separate",
            StagePatch {
                progress: Some(50.0),
                ..Default::default()
            },
        )
        .unwrap();

        let reread = read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].name, "separate");
        assert_eq!(st[0].status, StageStatus::Running);
        assert_eq!(st[0].progress, Some(50.0));
        assert_eq!(st[0].started_at.as_deref(), Some("2024-01-01T00:00:01Z"));

        // 标记 success 清空 error_message
        set_stage(
            &dir,
            "separate",
            StagePatch {
                status: Some(StageStatus::Success),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("boom".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st[0].status, StageStatus::Success);
        assert!(st[0].error_message.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_task_patch() {
        let dir = std::env::temp_dir()
            .join(format!("ld_task_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "pipeline": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        set_task(
            &dir,
            TaskPatch {
                status: Some("success".into()),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("stale".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        assert_eq!(reread.task.status, "success");
        assert!(reread.task.error_message.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_helpers() {
        let d = "/work/task123";
        assert_eq!(separate_dir(d), Path::new("/work/task123/separate"));
        assert_eq!(
            vocals_path(d),
            Path::new("/work/task123/separate/target_3_vocals.wav")
        );
        assert_eq!(task_id(d).as_deref(), Some("task123"));
    }
}
