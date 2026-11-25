//! enqueue_dir 扫描逻辑 (纯函数, 可独立测试)。
//!
//! 扫描本地目录**顶层**视频文件, 每个并行:
//! - 用 `auto_group_id_and_video_id` 派生 group/video_id;
//! - 若 `workfolder/<group>/<video_id>/view.json` 已存在 → 跳过 (幂等，已导入过，不重复导入);
//! - 否则基于 base_input 生成一条 action=import_and_start 的 Input (覆盖 url=绝对路径)。
//!
//! 返回准备好的 Input 列表 + 摘要。队列入队动作由调用方 (server RPC) 执行。

use std::path::Path;

use crate::input::Input;
use crate::workflow::args::WorkflowAction;
use crate::workflow::import::util::mime_type::VIDEO_EXTENSIONS;
use crate::workflow::import::util::auto_group_id_and_video_id;

use super::get_workflow::EnqueueDirResult;

pub struct DirScanOutput {
    pub inputs: Vec<Input>,
    pub result: EnqueueDirResult,
}

/// 扫描目录, 生成待入队的 start Input 列表 (不包含跳过项)。
pub fn scan_dir_videos(dir: &Path, wf_root: &Path, base_input: &Input) -> DirScanOutput {
    let mut inputs = Vec::new();
    let mut scanned = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    let mut skipped_videos: Vec<String> = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return DirScanOutput {
            inputs,
            result: EnqueueDirResult {
                scanned,
                enqueued: 0,
                skipped,
                errors: 1,
                skipped_videos,
            },
        };
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_video = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| {
                let e = ext.to_lowercase();
                VIDEO_EXTENSIONS.iter().any(|&x| x == e)
            })
            .unwrap_or(false);
        if !is_video {
            continue;
        }
        scanned += 1;
        let abs = path.to_string_lossy().into_owned();

        match auto_group_id_and_video_id(&abs) {
            Ok(info) => {
                if wf_root
                    .join(&info.group_id)
                    .join(&info.video_id)
                    .join("view.json")
                    .exists()
                {
                    skipped += 1;
                    skipped_videos.push(info.video_id);
                    continue;
                }
                let mut video_input = base_input.clone();
                if let Some(ref mut w) = video_input.workflow {
                    w.url = Some(abs);
                    // TODO(user): enqueue 的 import 拆分先保持旧行为，之后再分。
                    w.action = Some(WorkflowAction::ImportAndStart);
                    w.video_dir = None;
                }
                inputs.push(video_input);
            }
            Err(_) => errors += 1,
        }
    }

    let enqueued = inputs.len() as u32;
    DirScanOutput {
        inputs,
        result: EnqueueDirResult {
            scanned,
            enqueued,
            skipped,
            errors,
            skipped_videos,
        },
    }
}

/// 本地批量执行结果（与队列的 EnqueueDirResult 对应，但 done/failed 代替 enqueued）。
#[derive(Debug, Clone)]
pub struct LocalBatchResult {
    pub scanned: u32,
    pub done: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errors: u32,
    pub failed_videos: Vec<String>,
}

/// 本地串行扫描并执行目录里每个视频：与队列 worker 一致——逐条独立，某条失败
/// 记录后继续下一条。与 `enqueue_dir`（发 RPC 入队）是同一动作的两种执行方式。
pub async fn run_dir_locally(input: &Input) -> anyhow::Result<LocalBatchResult> {
    let dir_str = input
        .workflow
        .as_ref()
        .and_then(|w| w.url.as_deref())
        .ok_or_else(|| anyhow::anyhow!("import_and_start_by_dir 需要 workflow.url (本地目录路径)"))?;
    let dir = std::path::Path::new(dir_str)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("目录不存在或无法访问 ({dir_str}): {e}"))?;
    if !dir.is_dir() {
        return Err(anyhow::anyhow!("{dir_str} 不是目录"));
    }

    let wf_root = config_rs::path::paths::workfolder();
    let DirScanOutput { inputs, result } = scan_dir_videos(&dir, &wf_root, input);
    let mut out = LocalBatchResult {
        scanned: result.scanned,
        done: 0,
        failed: 0,
        skipped: result.skipped,
        errors: result.errors,
        failed_videos: Vec::new(),
    };

    for video_input in inputs {
        let name = video_input
            .workflow
            .as_ref()
            .and_then(|w| w.url.as_deref())
            .unwrap_or("?")
            .to_string();
        match super::import_and_start_workflow(&video_input).await {
            Ok(_) => {
                out.done += 1;
                tracing::info!(target: "batch", "完成: {name}");
            }
            Err(e) => {
                out.failed += 1;
                out.failed_videos.push(name.clone());
                // 与队列 worker 一致：失败不阻断后续条目。
                tracing::error!(target: "batch", "失败: {name}: {e:#}");
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir()
            .join(format!("enqueue_dir_test_{name}_{:?}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn create_video(path: &Path) {
        std::fs::write(path, b"fake-video").unwrap();
    }

    #[test]
    fn scan_returns_video_inputs() {
        let dir = tmp_dir("videos");
        create_video(&dir.join("ep01.mp4"));
        create_video(&dir.join("ep02.mkv"));
        let wf = config_rs::path::paths::workfolder();
        let out = scan_dir_videos(&dir, &wf, &Input::default());
        assert_eq!(out.result.scanned, 2);
        assert_eq!(out.result.enqueued, 2);
        assert_eq!(out.inputs.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_ignores_non_video_files() {
        let dir = tmp_dir("mixed");
        create_video(&dir.join("ep01.mp4"));
        std::fs::write(dir.join("readme.txt"), b"skip").unwrap();
        let wf = config_rs::path::paths::workfolder();
        let out = scan_dir_videos(&dir, &wf, &Input::default());
        assert_eq!(out.result.scanned, 1);
        assert_eq!(out.result.enqueued, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_skips_existing_state_json() {
        let dir = tmp_dir("existing");
        create_video(&dir.join("ep01.mp4"));
        let wf = config_rs::path::paths::workfolder();

        let dir_stem = dir.file_name().unwrap().to_str().unwrap();
        let ctx_dir = wf.join(dir_stem).join("ep01");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(ctx_dir.join("view.json"), "{}").unwrap();

        let out = scan_dir_videos(&dir, &wf, &Input::default());
        assert_eq!(out.result.scanned, 1);
        assert_eq!(out.result.skipped, 1);
        assert_eq!(out.result.enqueued, 0);
        assert!(out.inputs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&ctx_dir.parent().unwrap());
    }
}
