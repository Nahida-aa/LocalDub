//! separate_after: 用分离出的 stems 重新混音 (BGM 重建 + sidechain/raw-sum + gate),
//! 提升后续 ASR / tts-vc 的人声质量。
//!
//! 镜像 TS `packages/core/stages/separate/after.ts` (stageSeparateAfter), 纯 ffmpeg 编排。

use crate::context::TaskCtx;
use crate::stages::asr::args::AsrArgs;
use crate::stages::utils::{
    StagePatch, StageStatus, emit_log, ffmpeg, now_iso, separate_after_dir, separate_dir,
    set_stage_anyhow,
};

/// 读取 asr 阶段配置 (缺省用 AsrArgs::default, 对齐 TS `?? 默认值`)。
fn read_asr_cfg(ctx: &TaskCtx) -> AsrArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("asr"))
        .and_then(|v| serde_json::from_value::<AsrArgs>(v.clone()).ok())
        .unwrap_or_default()
}

/// 入口 (镜像 TS `stageSeparateAfter`)。
pub fn stage_separate_after(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    emit_log("separate_after: start");

    set_stage_anyhow(
        &task_dir,
        "separate_after",
        StagePatch {
            last_message: Some("Mixing BGM & sidechain...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let sep_dir = separate_dir(&task_dir);
    let out_dir = separate_after_dir(&task_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 separate_after 目录失败: {e}"))?;

    let stems = [
        ("drums", sep_dir.join("target_0_drums.wav")),
        ("bass", sep_dir.join("target_1_bass.wav")),
        ("other", sep_dir.join("target_2_other.wav")),
        ("vocals", sep_dir.join("target_3_vocals.wav")),
    ];
    let vocals_path = stems[3].1.clone();
    let bgm_dst = out_dir.join("target_bgm.wav");
    let mixed_dst = out_dir.join("target_3_vocals_mixed.wav");

    // 1. 由 stems 重建 target_bgm.wav (修复 amix normalize bug)
    let all_stems_exist = stems[..3].iter().all(|(_, p)| p.exists());
    if all_stems_exist {
        emit_log("[SeparateAfter] Generating target_bgm.wav (amix normalize=0)...");
        ffmpeg(&[
            "-i".into(),
            stems[0].1.to_string_lossy().into_owned(),
            "-i".into(),
            stems[1].1.to_string_lossy().into_owned(),
            "-i".into(),
            stems[2].1.to_string_lossy().into_owned(),
            "-filter_complex".into(),
            "[0:a][1:a][2:a]amix=inputs=3:duration=first:normalize=0[out];[out]dynaudnorm=peak=0.5[final]".into(),
            "-map".into(),
            "[final]".into(),
            bgm_dst.to_string_lossy().into_owned(),
        ])?;
    } else if bgm_dst.exists() {
        emit_log("[SeparateAfter] target_bgm.wav exists, reusing (stems not all found)");
    } else {
        emit_log("[SeparateAfter] No stems or BGM found, skipping BGM generation");
    }

    // 2. 按 asr 配置生成 sidechain 混音人声
    let asr_cfg = read_asr_cfg(ctx);
    let use_separated = asr_cfg.use_separated;
    let mix_mode = asr_cfg.mix_mode;
    let reduce_bgm = asr_cfg.reduce_bgm;
    let sc = asr_cfg.sidechain_compress;
    let use_gate = asr_cfg.use_gate;

    if use_separated && !matches!(mix_mode, crate::stages::asr::args::MixMode::Vocals) {
        if !vocals_path.exists() {
            return Err(anyhow::anyhow!(
                "[SeparateAfter] vocals not found: {}",
                vocals_path.display()
            ));
        }
        if !bgm_dst.exists() {
            return Err(anyhow::anyhow!(
                "[SeparateAfter] BGM not found: {}",
                bgm_dst.display()
            ));
        }
        match mix_mode {
            crate::stages::asr::args::MixMode::RawSum => {
                emit_log(&format!(
                    "[SeparateAfter] raw-sum: mixing vocals + BGM at {reduce_bgm}dB..."
                ));
                ffmpeg(&[
                    "-i".into(),
                    vocals_path.to_string_lossy().into_owned(),
                    "-i".into(),
                    bgm_dst.to_string_lossy().into_owned(),
                    "-filter_complex".into(),
                    format!(
                        "[1:a]volume={reduce_bgm}dB[bgm_r];[0:a][bgm_r]amix=inputs=2:duration=first:weights=1 1[out]"
                    ),
                    "-map".into(),
                    "[out]".into(),
                    mixed_dst.to_string_lossy().into_owned(),
                ])?;
            }
            crate::stages::asr::args::MixMode::Sidechain => {
                let sc_params = format!(
                    "threshold={}:ratio={}:attack={}:release={}",
                    sc.threshold, sc.ratio, sc.attack, sc.release
                );
                let bgm_vol = if reduce_bgm != 0.0 {
                    format!(";[bgm_sc]volume={reduce_bgm}dB[bgm_final]")
                } else {
                    String::new()
                };
                let bgm_final = if reduce_bgm != 0.0 {
                    "bgm_final"
                } else {
                    "bgm_sc"
                };
                emit_log(&format!(
                    "[SeparateAfter] sidechain: {sc_params}, bgmReduce={reduce_bgm}dB"
                ));
                let filter = format!(
                    "[0:a]asplit[v][v_key];[1:a][v_key]sidechaincompress={sc_params}[bgm_sc]{bgm_vol};[v][{bgm_final}]amix=inputs=2:duration=first:weights=1 1[out]"
                );
                ffmpeg(&[
                    "-i".into(),
                    vocals_path.to_string_lossy().into_owned(),
                    "-i".into(),
                    bgm_dst.to_string_lossy().into_owned(),
                    "-filter_complex".into(),
                    filter,
                    "-map".into(),
                    "[out]".into(),
                    mixed_dst.to_string_lossy().into_owned(),
                ])?;
            }
            crate::stages::asr::args::MixMode::Vocals => unreachable!(),
        }
    }

    // 3. 可选 silence gate
    if use_gate && mixed_dst.exists() {
        let gated_path = out_dir.join("target_3_vocals_gated.wav");
        emit_log("[SeparateAfter] Applying silence gate...");
        ffmpeg(&[
            "-i".into(),
            mixed_dst.to_string_lossy().into_owned(),
            "-af".into(),
            "agate=threshold=0.02:ratio=20:attack=10:release=100".into(),
            gated_path.to_string_lossy().into_owned(),
        ])?;
    }

    set_stage_anyhow(
        &task_dir,
        "separate_after",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Done".into()),
            ..Default::default()
        },
    )?;
    emit_log("separate_after: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ffmpeg_available() -> bool {
        std::process::Command::new(crate::stages::utils::ffmpeg_bin())
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn make_silence_wav(path: &std::path::Path) {
        ffmpeg(&[
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "anullsrc=r=44100:cl=stereo".into(),
            "-t".into(),
            "1".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap();
    }

    fn ctx_at(dir: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = dir.to_string();
        ctx.task.id = "t".to_string();
        ctx.pipeline = "dub".to_string();
        ctx
    }

    #[test]
    fn separate_after_builds_bgm_and_sidechain() {
        if !ffmpeg_available() {
            eprintln!("ffmpeg 不可用, 跳过 separate_after 集成测试");
            return;
        }
        let dir = std::env::temp_dir()
            .join(format!("ld_sep_after_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(separate_dir(&dir)).unwrap();
        std::fs::create_dir_all(separate_after_dir(&dir)).unwrap();

        for (i, name) in ["drums", "bass", "other", "vocals"].iter().enumerate() {
            make_silence_wav(&separate_dir(&dir).join(format!("target_{i}_{name}.wav")));
        }

        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"asr": {"useSeparated": true, "mixMode": "sidechain",
                                            "reduceBgm": -12, "useGate": true}}}
            }),
        );
        crate::context::write_ctx(&dir, &ctx).unwrap();

        let res = stage_separate_after(&ctx);
        assert!(res.is_ok(), "separate_after 失败: {:?}", res.err());

        assert!(
            separate_after_dir(&dir).join("target_bgm.wav").exists(),
            "target_bgm.wav 应生成"
        );
        assert!(
            separate_after_dir(&dir)
                .join("target_3_vocals_mixed.wav")
                .exists(),
            "target_3_vocals_mixed.wav 应生成"
        );
        assert!(
            separate_after_dir(&dir)
                .join("target_3_vocals_gated.wav")
                .exists(),
            "useGate=true 时 target_3_vocals_gated.wav 应生成"
        );

        let reread = crate::context::read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st[0].name, "separate_after");
        assert_eq!(st[0].status, StageStatus::Success);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
