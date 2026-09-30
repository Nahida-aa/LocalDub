//! mix_video 阶段的产物事实与成片路径推导。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::input::Input;
use crate::steps::utils::{final_video_dir, video_id};
use crate::workflows::args::{OutputMode, SubtitleSource};

/// `<mix_video>/out.json`：烧字幕完成后登记的**成片事实**。
///
/// 保守地带上路径自我描述（`final_video_path` + `final_dir`）——下游与导出脚本
/// 直接读它，不必再复述一遍「目录名怎么拼」的规则。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MixVideoOutput {
    /// 成片的绝对路径。
    pub final_video_path: PathBuf,
    /// 成片子目录名（`dub` / `dub_sf_ocr` / `subtitle_ntl` …）。
    ///
    /// 留档用：便于从任务目录反查「这是按哪种组合烧出来的」。
    pub final_dir: String,
}

impl MixVideoOutput {
    /// 登记成片（mix_video 成功写出 mp4 后调用，唯一写者）。
    pub fn write(video_dir: &str, final_video: &Path) -> Result<Self> {
        // final_dir 取 `<video_dir>/mix_video/<name>` 的最后一段（落在 `<name>.mp4`
        // 的父目录名上），避免在这里复述一遍目录名拼法规则。
        let final_dir = final_video
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let out = Self {
            final_video_path: final_video.to_path_buf(),
            final_dir,
        };
        crate::state::write_step_out(video_dir, "mix_video", &out)
            .map_err(anyhow::Error::msg)
            .context("写 <mix_video>/out.json 失败")?;
        Ok(out)
    }

    /// 读 `<mix_video>/out.json`；还没烧完 → `None`。
    pub fn read(video_dir: &str) -> Result<Option<Self>> {
        crate::state::read_step_out(video_dir, "mix_video").map_err(anyhow::Error::msg)
    }
}

/// 成片路径的**纯推导**——只依赖 `video_dir` + `input`。
///
/// 与 [`MixVideoOutput::read`] 的区别：这条不读盘、也不要求 mix_video 跑过，
/// 任何时刻对任何任务都能算出「按当前 input 会烧到哪」。代价是它回答的是
/// 「按现在的配置」而非「当时真的烧在哪」——任务跑到一半改 input 时两者会分叉。
pub fn final_video_path(video_dir: &str, input: &Input) -> PathBuf {
    let video_id = video_id(video_dir).unwrap_or_default();
    PathBuf::from(video_dir)
        .join("mix_video")
        .join(final_video_dir(
            OutputMode::from_input(input).as_str(),
            SubtitleSource::from_input(input)
                .map(|s| s.as_str())
                .unwrap_or("asr"),
            !input.steps.translate.enabled,
        ))
        .join(format!("{video_id}.mp4"))
}
