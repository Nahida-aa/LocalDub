//! enqueue_dir 扫描逻辑 (纯函数, 可独立测试)。
//!
//! 扫描本地目录**顶层**视频文件, 每个并行:
//! - 用 `auto_group_id_and_video_id` 派生 group/video_id;
//! - 若 `workfolder/<group>/<video_id>/ctx.json` 已存在 → 跳过 (幂等);
//! - 否则基于 base_input 生成一条 action=start 的 Input (覆盖 url=绝对路径)。
//!
//! 返回准备好的 Input 列表 + 摘要。队列入队动作由调用方 (server RPC) 执行。

use std::path::Path;

use crate::input::Input;
use crate::workflows::args::WorkflowAction;
use crate::workflows::import::util::mime_type::VIDEO_EXTENSIONS;
use crate::workflows::import::util::auto_group_id_and_video_id;

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
                if wf_root.join(&info.group_id).join(&info.video_id).join("ctx.json").exists() {
                    skipped += 1;
                    skipped_videos.push(info.video_id);
                    continue;
                }
                let mut video_input = base_input.clone();
                if let Some(ref mut w) = video_input.workflow {
                    w.url = Some(abs);
                    w.action = Some(WorkflowAction::Start);
                    w.workflow_dir = None;
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
    fn scan_skips_existing_ctx_json() {
        let dir = tmp_dir("existing");
        create_video(&dir.join("ep01.mp4"));
        let wf = config_rs::path::paths::workfolder();

        let dir_stem = dir.file_name().unwrap().to_str().unwrap();
        let ctx_dir = wf.join(dir_stem).join("ep01");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(ctx_dir.join("ctx.json"), "{}").unwrap();

        let out = scan_dir_videos(&dir, &wf, &Input::default());
        assert_eq!(out.result.scanned, 1);
        assert_eq!(out.result.skipped, 1);
        assert_eq!(out.result.enqueued, 0);
        assert!(out.inputs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&ctx_dir.parent().unwrap());
    }
}