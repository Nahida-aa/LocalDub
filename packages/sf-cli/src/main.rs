//! 关键帧 OCR 策略入口：subtitle-finder 吃全部帧 → 找关键帧（每字幕段 1 张，
//! 自带 start_ms/end_ms）→ 只对关键帧跑 subtitle-ocr。对比逐帧 2fps OCR 可大幅
//! 减少识别调用，且时间轴由关键帧直接给出。
//!
//! 用法：sf-ocr <video.mp4>（OCR_MODELS_DIR 指向 rapidocr 模型目录）
//! 例： OCR_MODELS_DIR=data/models/rapidocr sf-ocr ref/video_source.mp4

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rapidocr_ort::ModelProfile;
use subtitle_ocr::{OcrOptions, SubtitleOcr};

fn main() -> Result<()> {
    let video = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .context("用法: sf-ocr <video.mp4>")?;
    if !video.exists() {
        bail!("视频不存在: {}", video.display());
    }
    let model_dir = PathBuf::from(
        std::env::var("OCR_MODELS_DIR")
            .context("缺 OCR_MODELS_DIR（rapidocr 模型目录，如 data/models/rapidocr）")?,
    );

    let params = subtitle_finder::params::Params::default();
    let keyframes = subtitle_finder::find_keyframes(&video, &params)?;
    println!("keyframes: {}", keyframes.len());

    let mut ocr = SubtitleOcr::from_profile(ModelProfile::V3, &model_dir, OcrOptions::default())?;
    for (i, kf) in keyframes.iter().enumerate() {
        let lines = ocr.ocr_image(&kf.frame)?;
        let text = lines
            .iter()
            .map(|l| l.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let t = if text.is_empty() {
            "(未识别)".to_string()
        } else {
            text
        };
        println!("[{}] {}-{}ms: {}", i, kf.start_ms, kf.end_ms, t);
    }

    Ok(())
}
