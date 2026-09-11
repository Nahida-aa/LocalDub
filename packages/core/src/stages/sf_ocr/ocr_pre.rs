//! sf_ocr_pre: 关键帧策略前处理, 调 subtitle-finder (通过 env 管理) 找字幕关键帧。
//!
//! 镜像 TS `packages/core/stages/sf_ocr/ocr_pre.ts` (stageSfOcrPre)。
//! 落盘 `<workflowDir>/sf_ocr_pre/`: frames/(PNG) / mask/ / timeline.txt / keyframes.json。

use crate::cmd::env::ensure_bin;
use crate::context::WorkflowCtx;
use crate::stages::utils::{
    StagePatch, StageStatus, ensure_dir, now_iso,
    set_stage_anyhow, sf_ocr_pre_dir, video_source_path,
};
use std::process::Command;

/// 入口 (镜像 TS `stageSfOcrPre`)。
pub fn stage_sf_ocr_pre(ctx: &WorkflowCtx) -> anyhow::Result<()> {
    let workflow_dir = ctx.workflow.workflow_dir.clone();
    tracing::info!(target: "sf_ocr", "start");

    set_stage_anyhow(
        &workflow_dir,
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

    let bin = ensure_bin("subtitle_finder_bin").map_err(|e| {
        anyhow::anyhow!(
            "{e}\n若下载失败, 请手动执行: cargo run -p cli -- env --action ensure --targets subtitle_finder_bin"
        )
    })?;

    let out_dir = sf_ocr_pre_dir(&workflow_dir);
    ensure_dir(&out_dir)?;

    tracing::info!(target: "sf_ocr", "subtitle-finder {video_path} --out {}", out_dir.display());
    let status = Command::new(&bin)
        .arg(&video_path)
        .arg("--out")
        .arg(&out_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn subtitle-finder 失败: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "subtitle-finder failed with exit code {:?}",
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
        &workflow_dir,
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

    fn ctx_at(dir: &str) -> WorkflowCtx {
        let mut ctx = read_ctx_from_value(json!({
            "workflow": {"id":"t","workflow_dir":dir,"url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {}
        }))
        .unwrap();
        ctx.workflow.workflow_dir = dir.to_string();
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
        // 放一个真实存在的视频文件, 让 video 检查通过, 触发二进制缺失报错。
        // 预置 data/bin 一个"假" subtitle-finder + 版本戳, 让 check 通过 (避免触发真实下载),
        // 随后 spawn 在假视频上失败 → 报 subtitle-finder failed。
        //
        // 注意: env ensure 的落盘目录是真实 data/bin (bin_dir 无 env 覆盖), 故测试
        // 先备份现场、结束后恢复, 避免破坏真实下载的二进制。
        let dir = std::env::temp_dir()
            .join(format!("ld_sfpre_bin_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = format!("{dir}/video.mp4");
        std::fs::write(&video, b"fake").unwrap();

        // 备份 data/bin 现场
        let bin_dir = config_rs::path::models::bin_dir();
        std::fs::create_dir_all(&bin_dir).unwrap();
        let bin_path = bin_dir.join("subtitle-finder");
        let stamp_path = bin_dir.join(".subtitle_finder.version.json");
        let backup_bin = std::fs::read(&bin_path).ok();
        let backup_stamp = std::fs::read(&stamp_path).ok();
        let had_bin = bin_path.exists();
        let had_stamp = stamp_path.exists();

        // 预置假二进制 + 版本戳, 使 check 通过而无需网络
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 1\n").unwrap();
        let stamp = serde_json::json!({
            "tag": "subtitle-finder-v0.1.0",
            "sha256": "b08778b2e066a35f8c9b3c0457e3e05a1379a6452341b932d82c22175cba9923",
            "downloaded_at": "2026-09-08T00:00:00Z",
        });
        std::fs::write(
            &stamp_path,
            serde_json::to_string_pretty(&stamp).unwrap(),
        )
        .unwrap();

        let mut ctx = ctx_at(&dir);
        ctx.video_source_path = Some(video);
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_sf_ocr_pre(&ctx);

        // 清理假二进制, 恢复现场
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(&stamp_path);
        if had_bin {
            if let Some(bytes) = backup_bin {
                std::fs::write(&bin_path, bytes).unwrap();
            }
        }
        if had_stamp {
            if let Some(bytes) = backup_stamp {
                std::fs::write(&stamp_path, bytes).unwrap();
            }
        }

        assert!(res.is_err());
        let msg = res.unwrap_err().to_string();
        // 二进制就绪但视频非法 → subtitle-finder 运行失败
        assert!(
            msg.contains("subtitle-finder"),
            "应提示 subtitle-finder 运行失败, 实际: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
