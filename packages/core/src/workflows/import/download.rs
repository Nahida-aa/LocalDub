//! import 阶段: 视频导入工作区 (镜像 TS `packages/core/workflows/import/download.ts`)。
//!
//! 主流程: 判定来源 → 下载/复制 → 转码 mp4 + 抽音频 → 探测帧率 → 写状态文件。
//!
//! import 是任务目录的**创建者**, 也是 `state.json` / `input.json` 的唯一初次写入方:
//! 身份 (workflow 子树) 与 import 事实 (video/audio_source_path、frame_rate) 进
//! `state.json`, 配置进 `input.json` (两者写者与生命周期不同, 见 [`crate::state`])。
//! **不预写任何 `<step>/state.json`** —— 步骤状态文件由各步骤自己在首次运行时创建
//! (缺失即 pending)。

use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;
use tracing::{info, warn};

use crate::input::Input;
use crate::state::{write_input, write_state, VideoSource, Workflow, WorkflowState};
use crate::workflows::import::util::{
    auto_group_id_and_video_id, copy_file_to_path, download_remote_video, encode_to_mp4,
    extract_audio, probe_frame_rate, run_yt_dlp_download,
};

#[derive(Debug, Clone)]
pub struct Downloaded {
    pub downloaded_video_path: String,
    pub video_path: String,
    pub audio_path: String,
}

/// import 期构建 step 进度清单 (镜像 TS `getSteps`)。
///
/// 顶层导入入口 (镜像 TS `importVideo`)。
pub fn import_video(input: &Input) -> anyhow::Result<(WorkflowState, Input)> {
    let args = input.workflow.clone().unwrap_or_default();
    let url = args
        .url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("workflow start: 需要 workflow.url"))?;

    let auto = auto_group_id_and_video_id(&url)?;
    let group_id = auto.group_id;
    let video_id = auto.video_id;
    let source = auto.source;
    let yt_dlp_ext_args = auto.yt_dlp_ext_args;

    info!("[import] group={group_id} workflow={video_id} source={source:?}");
    let video_dir = workfolder().join(&group_id).join(&video_id);
    std::fs::create_dir_all(&video_dir)
        .with_context(|| format!("创建 videoDir 失败: {video_dir:?}"))?;

    let downloaded = download_video(
        &url,
        source,
        &group_id,
        &video_id,
        &yt_dlp_ext_args,
        args.download_subtitles,
    )?;

    // 探测帧率
    let frame_rate = probe_frame_rate(&downloaded.video_path);
    info!(
        "[import] 帧率: {}/{}",
        frame_rate.numerator, frame_rate.denominator
    );

    let output_mode = args.output_mode;
    let mode_str = serde_json::to_value(output_mode)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "dub".to_string());

    let state = WorkflowState {
        workflow: Workflow {
            id: video_id.clone(),
            source,
            url: url.clone(),
            title: auto.title.clone(),
            status: "queued".to_string(),
            current_step: None,
            video_dir: video_dir.to_string_lossy().into(),
            error_message: None,
            created_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
        },
        output_mode: mode_str,
        frame_rate,
        video_source_path: Some(downloaded.video_path.clone()),
        audio_source_path: Some(downloaded.audio_path.clone()),
    };

    let video_dir_str = video_dir.to_string_lossy().into_owned();
    write_state(&video_dir_str, &state).map_err(anyhow::Error::msg)?;
    // 身份回填：input.json 与返回值里的 videoDir 都填成 import 落定的真实目录
    // （start 路径的 input 原本没有它——此前只有引擎注入这一种补法）。落盘的
    // input.json 与返回的新 Input 同源同构：读盘的 = 调用方拿到的。
    let mut input = input.clone();
    if let Some(w) = input.workflow.as_mut() {
        w.video_dir = Some(video_dir_str.clone());
    }
    write_input(&video_dir_str, &input).map_err(anyhow::Error::msg)?;
    // 导入完成的用户可见提示（原 cmd 层 import_workflow 的打印，内化到干活的函数里）
    println!("导入完成, video_dir = {video_dir_str}");
    Ok((state, input))
}

/// 下载/复制视频并转码 + 抽音频 (镜像 TS `downloadVideo`)。
pub fn download_video(
    url: &str,
    source: VideoSource,
    group_id: &str,
    video_id: &str,
    yt_dlp_ext_args: &[String],
    download_subtitles: bool,
) -> anyhow::Result<Downloaded> {
    let video_dir = workfolder().join(group_id).join(video_id);
    let mut downloaded_video_path = video_dir.join(format!("{video_id}.mp4"));
    let video_path = video_dir.join("video_source.mp4");
    let audio_path = video_dir.join("audio_source.wav");

    match source {
        VideoSource::Local | VideoSource::Remote => {
            if matches!(source, VideoSource::Local) {
                copy_file_to_path(url, downloaded_video_path.to_str().unwrap())
                    .context("复制本地视频失败")?;
            } else {
                let raw = download_remote_video(url, video_dir.to_str().unwrap())
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
                video_dir
                    .join(format!("{video_id}.%(ext)s"))
                    .to_string_lossy()
                    .into(),
            ];
            yt_args.extend(yt_dlp_ext_args.iter().cloned());
            yt_args.push(url.to_string());

            crate::workflows::import::util::run_yt_dlp_download(&yt_args)
                .context("yt-dlp 下载失败")?;

            // 定位实际产物: yt-dlp 输出 `{video_id}.%(ext)s`, 产物即 `{video_id}.<ext>` (mp4/webm 等)。
            // 不重命名, 直接用实际文件 (ext 由 yt-dlp 决定, 程序可判断)。
            downloaded_video_path = find_workflow_video(&video_dir, video_id)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp 未产出 {video_id}.<ext>"))?;

            // 转码成标准 mp4 (video_source.mp4), 与本地分支命名一致
            encode_to_mp4(
                downloaded_video_path.to_str().unwrap(),
                video_path.to_str().unwrap(),
            )
            .context("转码失败")?;
            extract_audio(video_path.to_str().unwrap(), audio_path.to_str().unwrap())
                .context("抽取音频失败")?;

            // 可选: 下载平台自带字幕 (官方/自动) 落盘到 download/ 供后续分析/消费。
            // best-effort: YouTube 现需 PO token, 无服务时失败仅告警, 不阻断主流程。
            if download_subtitles {
                if let Err(e) = download_youtube_subtitles(url, &video_dir, yt_dlp_ext_args) {
                    warn!("[import] 字幕下载失败 (已忽略): {e}");
                }
            }
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

/// 在 video_dir 里找 `{video_id}.<ext>` 的实际下载产物 (yt-dlp 输出 `{video_id}.%(ext)s`)。
fn find_workflow_video(video_dir: &std::path::Path, video_id: &str) -> Option<PathBuf> {
    let prefix = format!("{video_id}.");
    std::fs::read_dir(video_dir).ok()?.find_map(|e| {
        let p = e.ok()?.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        (name.starts_with(&prefix) && p.is_file()).then_some(p)
    })
}

/// 下载平台自带字幕 (YouTube/Bilibili 的官方 + 自动字幕) 落盘到 `video_dir/download/`。
///
/// best-effort: YouTube 现要求 PO token, 无 bgutil 服务时 yt-dlp 会报
/// "missing subtitles because a PO token was not provided", 调用方应捕获并告警。
/// 下载产物形如 `download/{video_id}.{lang}.vtt` / `.srt` (由 yt-dlp 决定)。
fn download_youtube_subtitles(
    url: &str,
    video_dir: &std::path::Path,
    yt_dlp_ext_args: &[String],
) -> anyhow::Result<()> {
    let out_dir = video_dir.join("download");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("创建 {} 失败", out_dir.display()))?;

    // 复用主下载的 cookies/proxy/playlist 参数; 额外加字幕参数。
    // --write-subs: 官方字幕; --write-auto-subs: 自动生成字幕; 全部语言。
    // 不另加 --no-playlist: 沿用 yt_dlp_ext_args 里已有的 playlist 处理 (单集/整列一致)。
    let mut args: Vec<String> = vec![
        "--skip-download".into(),
        "--write-subs".into(),
        "--write-auto-subs".into(),
        "--sub-langs".into(),
        "all".into(),
        "-o".into(),
        out_dir
            .join("%(id)s.%(lang)s.%(ext)s")
            .to_string_lossy()
            .into(),
    ];
    args.extend(yt_dlp_ext_args.iter().cloned());
    args.push(url.to_string());

    info!("[import] 下载字幕: yt-dlp {:?}", args);
    run_yt_dlp_download(&args).context("yt-dlp 字幕下载失败")?;
    info!("[import] 字幕下载完成, 落盘于 {}", out_dir.display());
    Ok(())
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
        // canonicalize 消掉路径里的字面 ".."：parse_dir_and_id 把 ".." 当目录名
        // 会解析出 group=".."，video_dir 逃出 workfolder（曾把仓库根 smoke/ 清掉）。
        let smoke = std::fs::canonicalize(smoke)
            .expect("canonicalize smoke.mp4 失败")
            .to_string_lossy()
            .into_owned();
        let input = Input {
            workflow: Some(crate::workflows::args::WorkflowArgs {
                url: Some(smoke.to_string()),
                output_mode: crate::workflows::args::OutputMode::Dub,
                subtitle_source: crate::workflows::args::SubtitleSource::Asr,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (st, imported) = import_video(&input).expect("import_video 失败");
        assert!(st
            .video_source_path
            .as_ref()
            .unwrap()
            .ends_with("video_source.mp4"));
        // 返回的 Input 已回填 videoDir（与 input.json 落盘内容一致）
        assert_eq!(
            imported
                .workflow
                .as_ref()
                .and_then(|w| w.video_dir.as_deref()),
            Some(st.workflow.video_dir.as_str())
        );
        assert!(st
            .audio_source_path
            .as_ref()
            .unwrap()
            .ends_with("audio_source.wav"));
        assert!(std::path::Path::new(st.video_source_path.as_ref().unwrap()).exists());
        assert!(std::path::Path::new(st.audio_source_path.as_ref().unwrap()).exists());
        // import 写出的状态产出物: state.json (身份+事实) + input.json (配置)
        // ——各步骤的 state.json 不预写。
        let dir = st.workflow.video_dir.as_str();
        let state_path = crate::state::state_path(dir);
        let input_path = crate::state::input_path(dir);
        assert!(
            state_path.exists(),
            "import 未写出 {}",
            state_path.display()
        );
        assert!(
            input_path.exists(),
            "import 未写出 {}",
            input_path.display()
        );
        let reread = crate::state::read_state(dir).expect("read_state 失败");
        assert_eq!(reread.workflow.id, st.workflow.id);
        let reread_input = crate::state::read_input(dir).expect("read_input 失败");
        assert_eq!(
            reread_input
                .workflow
                .as_ref()
                .and_then(|w| w.url.clone()),
            Some(smoke.to_string())
        );
        // input.json 落盘的也是回填过 videoDir 的版本
        assert_eq!(
            reread_input
                .workflow
                .as_ref()
                .and_then(|w| w.video_dir.clone()),
            Some(st.workflow.video_dir.clone())
        );
        // 清理: 删除生成的 workflow 目录
        let _ = std::fs::remove_dir_all(&st.workflow.video_dir);
        println!(
            "import 成功: group={} workflow={}",
            st.workflow.id, st.workflow.id
        );
    }
}
