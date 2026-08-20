//! import 阶段: 视频导入工作区 (镜像 TS `packages/core/tasks/import/download.ts`)。
//!
//! 主流程: 判定来源 → 下载/复制 → 转码 mp4 + 抽音频 → 探测帧率 → 写 TaskCtx。

use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;
use tracing::info;

use crate::context::{Task, TaskCtx, TaskStage, VideoSource, read_ctx_from_value, write_ctx};
use crate::input::Input;
use crate::tasks::args::Pipeline;
use crate::tasks::import::util::{
    auto_group_id_and_video_id, copy_file_to_path, download_remote_video, encode_to_mp4,
    extract_audio, probe_frame_rate,
};

#[derive(Debug, Clone)]
pub struct Downloaded {
    pub downloaded_video_path: String,
    pub video_path: String,
    pub audio_path: String,
}

/// 构建 pipeline 对应的 stage 列表 (镜像 TS `getStages`)。
///
/// 与 TS 不同: 不读磁盘 input.json, 直接从传入的 pipeline / subtitle_source 计算,
/// 使 import 自包含。translate / split_audio 的开关依赖 stage 配置, 这里保持默认
/// 全量 (与 TS 无 config 时的 fallback 一致)。
pub fn get_stages(
    pipeline: Pipeline,
    subtitle_source: crate::tasks::args::SubtitleSource,
) -> Vec<String> {
    let mut stages: Vec<&str> = match subtitle_source {
        crate::tasks::args::SubtitleSource::SfOcr => vec![
            "separate",
            "separate_after",
            "sf_ocr_pre",
            "sf_ocr",
            "sf_ocr_fix",
            "translate",
            "split_audio",
            "tts",
            "mix_audio",
            "mix_video",
        ],
        crate::tasks::args::SubtitleSource::AsrOcr => vec![
            "separate",
            "separate_after",
            "asr",
            "asr_ocr_pre",
            "asr_ocr",
            "asr_ocr_fix",
            "translate",
            "split_audio",
            "tts",
            "mix_audio",
            "mix_video",
        ],
        crate::tasks::args::SubtitleSource::Asr => {
            if matches!(pipeline, Pipeline::Subtitle) {
                vec![
                    "separate",
                    "separate_after",
                    "asr",
                    "asr_fix",
                    "translate",
                    "split_audio",
                    "mix_video",
                ]
            } else {
                vec![
                    "separate",
                    "separate_after",
                    "asr",
                    "asr_fix",
                    "translate",
                    "split_audio",
                    "tts",
                    "mix_audio",
                    "mix_video",
                ]
            }
        }
    };
    stages.retain(|s| *s != "translate" || true); // 默认保留 translate
    stages.into_iter().map(|s| s.to_string()).collect()
}

/// 顶层导入入口 (镜像 TS `importVideo`)。
pub fn import_video(input: &Input) -> anyhow::Result<TaskCtx> {
    let args = input.task.clone().unwrap_or_default();
    let url = args
        .url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("task start: 需要 task.url"))?;

    let auto = auto_group_id_and_video_id(&url)?;
    let group_id = auto.group_id;
    let task_id = auto.task_id;
    let source = auto.source;
    let yt_dlp_ext_args = auto.yt_dlp_ext_args;

    info!("[import] group={group_id} task={task_id} source={source:?}");
    let task_dir = workfolder().join(&group_id).join(&task_id);
    std::fs::create_dir_all(&task_dir)
        .with_context(|| format!("创建 taskDir 失败: {task_dir:?}"))?;

    let downloaded = download_video(&url, source, &group_id, &task_id, &yt_dlp_ext_args)?;

    // 探测帧率
    let frame_rate = probe_frame_rate(&downloaded.video_path);
    info!(
        "[import] 帧率: {}/{}",
        frame_rate.numerator, frame_rate.denominator
    );

    let pipeline = args.pipeline;
    let subtitle_source = args.subtitle_source;
    let pipeline_str = serde_json::to_value(pipeline)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "dub".to_string());
    let stage_names = get_stages(pipeline, subtitle_source);
    let stages: Vec<TaskStage> = stage_names
        .iter()
        .map(|name| TaskStage {
            name: name.clone(),
            label: name.clone(),
            status: crate::context::StageStatus::Pending,
            progress: None,
            started_at: None,
            completed_at: None,
            last_message: None,
            error_message: None,
        })
        .collect();

    let ctx = TaskCtx {
        task: Task {
            id: task_id.clone(),
            source,
            url: url.clone(),
            title: auto.title.clone(),
            status: "queued".to_string(),
            current_stage: None,
            task_dir: task_dir.to_string_lossy().into(),
            final_video_path: None,
            error_message: None,
            created_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
        },
        stages: Some(stages),
        pipeline: pipeline_str.clone(),
        last_run_pipeline: Some(pipeline_str),
        // import 阶段 input 直接透传原始 Input (序列化为 Value)
        input: serde_json::to_value(input).unwrap_or(serde_json::Value::Null),
        frame_rate,
        run_info: None,
        video_source_path: Some(downloaded.video_path.clone()),
        audio_source_path: Some(downloaded.audio_path.clone()),
        asr_language: args.source_lang.and_then(|l| {
            serde_json::to_value(l)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
        }),
        target_language: args.target_lang.and_then(|l| {
            serde_json::to_value(l)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
        }),
    };

    write_ctx(&task_dir.to_string_lossy(), &ctx).map_err(anyhow::Error::msg)?;
    Ok(ctx)
}

/// 下载/复制视频并转码 + 抽音频 (镜像 TS `downloadVideo`)。
pub fn download_video(
    url: &str,
    source: VideoSource,
    group_id: &str,
    task_id: &str,
    yt_dlp_ext_args: &[String],
) -> anyhow::Result<Downloaded> {
    let task_dir = workfolder().join(group_id).join(task_id);
    let mut downloaded_video_path = task_dir.join(format!("{task_id}.mp4"));
    let video_path = task_dir.join("video_source.mp4");
    let audio_path = task_dir.join("audio_source.wav");

    match source {
        VideoSource::Local | VideoSource::Remote => {
            if matches!(source, VideoSource::Local) {
                copy_file_to_path(url, downloaded_video_path.to_str().unwrap())
                    .context("复制本地视频失败")?;
            } else {
                let raw = download_remote_video(url, task_dir.to_str().unwrap())
                    .context("远程下载失败")?;
                // 远程下载产物可能已是 mp4, 仍统一转码
                if raw != downloaded_video_path.to_string_lossy() {
                    copy_file_to_path(&raw, downloaded_video_path.to_str().unwrap())
                        .context("移动远程下载产物失败")?;
                }
            }
            encode_to_mp4(
                downloaded_video_path.to_str().unwrap(),
                video_path.to_str().unwrap(),
            )
            .context("转码失败")?;
            extract_audio(video_path.to_str().unwrap(), audio_path.to_str().unwrap())
                .context("抽取音频失败")?;
        }
        VideoSource::Youtube | VideoSource::Bilibili => {
            let mut yt_args: Vec<String> = vec![
                "-f".into(),
                "bestaudio[ext=m4a]+bestvideo[ext=mp4]/best[ext=mp4]/best".into(),
                "--merge-output-format".into(),
                "mp4".into(),
                "-o".into(),
                task_dir
                    .join(format!("{task_id}.%(ext)s"))
                    .to_string_lossy()
                    .into(),
            ];
            yt_args.extend(yt_dlp_ext_args.iter().cloned());
            yt_args.push(url.to_string());

            crate::tasks::import::util::run_yt_dlp_download(&yt_args).context("yt-dlp 下载失败")?;

            // 定位实际产物: yt-dlp 输出 `{task_id}.%(ext)s`, 产物即 `{task_id}.<ext>` (mp4/webm 等)。
            // 不重命名, 直接用实际文件 (ext 由 yt-dlp 决定, 程序可判断)。
            downloaded_video_path = find_task_video(&task_dir, task_id)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp 未产出 {task_id}.<ext>"))?;

            // 转码成标准 mp4 (video_source.mp4), 与本地分支命名一致
            encode_to_mp4(
                downloaded_video_path.to_str().unwrap(),
                video_path.to_str().unwrap(),
            )
            .context("转码失败")?;
            extract_audio(video_path.to_str().unwrap(), audio_path.to_str().unwrap())
                .context("抽取音频失败")?;
        }
        VideoSource::Unknown => {
            return Err(anyhow::anyhow!("未知视频来源, 无法下载"));
        }
    }

    Ok(Downloaded {
        downloaded_video_path: downloaded_video_path.to_string_lossy().into(),
        video_path: video_path.to_string_lossy().into(),
        audio_path: audio_path.to_string_lossy().into(),
    })
}

fn workfolder() -> PathBuf {
    config_rs::path::paths::workfolder()
}

/// 在 task_dir 里找 `{task_id}.<ext>` 的实际下载产物 (yt-dlp 输出 `{task_id}.%(ext)s`)。
fn find_task_video(task_dir: &std::path::Path, task_id: &str) -> Option<PathBuf> {
    let prefix = format!("{task_id}.");
    std::fs::read_dir(task_dir)
        .ok()?
        .find_map(|e| {
            let p = e.ok()?.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            (name.starts_with(&prefix) && p.is_file()).then_some(p)
        })
}

/// 从 ctx.json 读回 (方便下游阶段复用, 镜像 TS `readCtx`)。
pub fn read_ctx(task_dir: &str) -> anyhow::Result<TaskCtx> {
    let raw = std::fs::read_to_string(crate::context::ctx_path(task_dir))
        .with_context(|| format!("读 ctx.json 失败: {task_dir}"))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    read_ctx_from_value(v).map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Input;

    #[test]
    fn import_video_local_smoke() {
        // 需要一个本地 mp4; 用 packages/tmp/smoke.mp4 (由 ffmpeg 生成)
        let smoke = concat!(env!("CARGO_MANIFEST_DIR"), "/../tmp/smoke.mp4");
        if !std::path::Path::new(smoke).exists() {
            eprintln!("跳过: 无 {smoke}");
            return;
        }
        let input = Input {
            task: Some(crate::tasks::args::TaskArgs {
                url: Some(smoke.to_string()),
                pipeline: Pipeline::Dub,
                subtitle_source: crate::tasks::args::SubtitleSource::Asr,
                ..Default::default()
            }),
            ..Default::default()
        };
        let ctx = import_video(&input).expect("import_video 失败");
        assert!(
            ctx.video_source_path
                .as_ref()
                .unwrap()
                .ends_with("video_source.mp4")
        );
        assert!(
            ctx.audio_source_path
                .as_ref()
                .unwrap()
                .ends_with("audio_source.wav")
        );
        assert!(std::path::Path::new(ctx.video_source_path.as_ref().unwrap()).exists());
        assert!(std::path::Path::new(ctx.audio_source_path.as_ref().unwrap()).exists());
        // 清理: 删除生成的 task 目录
        let _ = std::fs::remove_dir_all(ctx.task.task_dir);
        println!("import 成功: group={} task={}", ctx.task.id, ctx.task.id);
    }
}
