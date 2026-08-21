//! sf_ocr_pre: 关键帧策略前处理, 调 sf-cli 找字幕关键帧。
//!
//! 镜像 TS `packages/core/stages/sf_ocr/ocr_pre.ts` (stageSfOcrPre)。
//! 落盘 `<taskDir>/sf_ocr_pre/`: frames/(PNG) / mask/ / timeline.txt / keyframes.json。

use crate::context::TaskCtx;
use crate::stages::utils::{
    StagePatch, StageStatus, cargo_build_bin, ensure_dir, find_release_bin, now_iso,
    set_stage_anyhow, sf_ocr_pre_dir, video_source_path,
};
use std::process::Command;

/// 入口 (镜像 TS `stageSfOcrPre`)。
pub fn stage_sf_ocr_pre(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    tracing::info!(target: "sf_ocr", "start");

    set_stage_anyhow(
        &task_dir,
        "sf_ocr_pre",
        StagePatch {
            last_message: Some("查找字幕关键帧...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let video_path = video_source_path(ctx)?;
    if !std::path::Path::new(&video_path).exists() {
        return Err(anyhow::anyhow!("OCR input not found: {video_path}"));
    }

    let bin = match find_release_bin("sf-cli") {
        Some(p) => p,
        None => {
            // 阶段内自动编译缺失二进制 (用户选项: 阶段内自动编译)
            tracing::info!(target: "sf_ocr", "未找到 sf-cli, 尝试自动编译...");
            cargo_build_bin("sf-cli", "sf-cli", &[], true).map_err(|e| {
                anyhow::anyhow!(
                    "{e}\n若编译失败, 请手动执行: cargo build --release -p sf-cli --bin sf-cli"
                )
            })?
        }
    };

    let out_dir = sf_ocr_pre_dir(&task_dir);
    ensure_dir(&out_dir)?;

    tracing::info!(target: "sf_ocr", "sf-cli {video_path} --out {}", out_dir.display());
    let status = Command::new(&bin)
        .arg(&video_path)
        .arg("--out")
        .arg(&out_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn sf-cli 失败: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "sf-cli failed with exit code {:?}",
            status.code()
        ));
    }

    let frame_dir = out_dir.join("frames");
    if !frame_dir.exists() {
        return Err(anyhow::anyhow!(
            "sf-cli 未产出关键帧目录: {}",
            frame_dir.display()
        ));
    }
    let kf_json = out_dir.join("keyframes.json");
    let keyframes: serde_json::Value = if kf_json.exists() {
        let raw = std::fs::read_to_string(&kf_json)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", kf_json.display(), e))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", kf_json.display(), e))?
    } else {
        serde_json::Value::Array(vec![])
    };
    let n = keyframes.as_array().map(|a| a.len()).unwrap_or(0);
    tracing::info!(target: "sf_ocr", 
        "[sf_ocr_pre] {n} keyframes -> {}",
        out_dir.display()
    );

    set_stage_anyhow(
        &task_dir,
        "sf_ocr_pre",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some(format!("找到 {n} 个关键帧")),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "sf_ocr", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_at(dir: &str) -> TaskCtx {
        let mut ctx = read_ctx_from_value(json!({
            "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {}
        }))
        .unwrap();
        ctx.task.task_dir = dir.to_string();
        ctx.pipeline = "dub".to_string();
        ctx
    }

    #[test]
    fn missing_video_errors_before_bin() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sfpre_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut ctx = ctx_at(&dir);
        ctx.video_source_path = Some(format!("{dir}/nope.mp4"));
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_sf_ocr_pre(&ctx);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("OCR input not found"),
            "应报视频缺失"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_bin_reports_unbuilt() {
        // 放一个真实存在的视频文件, 让 video 检查通过, 触发二进制缺失报错
        let dir = std::env::temp_dir()
            .join(format!("ld_sfpre_bin_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = format!("{dir}/video.mp4");
        std::fs::write(&video, b"fake").unwrap();
        let mut ctx = ctx_at(&dir);
        ctx.video_source_path = Some(video);
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_sf_ocr_pre(&ctx);
        assert!(res.is_err());
        let msg = res.unwrap_err().to_string();
        // 二进制缺失时 → "sf-cli 未构建"; 已构建但视频非法则 → "sf-cli failed"
        assert!(
            msg.contains("sf-cli 未构建") || msg.contains("sf-cli failed"),
            "应报 sf-cli 未构建或运行失败, 实际: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
