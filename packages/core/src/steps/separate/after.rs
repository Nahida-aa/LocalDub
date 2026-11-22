//! separate_after: 用分离出的 stems 重新混音 (BGM 重建 + sidechain/raw-sum + gate),
//! 提升后续 ASR / tts-vc 的人声质量。
//!
//! 镜像 TS `packages/core/steps/separate/after.ts` (stepSeparateAfter), 纯 ffmpeg 编排。

use crate::steps::separate::after_args::{
    MixMode, SeparateAfterDeps, SeparateAfterOutput,
};
use crate::steps::utils::paths::separate_after::{GatedVocalsPath, Target3VocalsMixedPath};
use crate::steps::utils::{
    ffmpeg, now_iso, separate_after_dir, set_step_anyhow, StepPatch, StepStatus,
};
use crate::workflows::step_ctx::{CtxView, StepCtxView};

/// 入口 (镜像 TS `stepSeparateAfter`)。
///
/// 意图面经 `sc.ctx().input()` 取（args / video_dir / use_separated），事实面由装配层
/// 算好注入 [`SeparateAfterDeps`]（separate 产出的 vocals_path / stems_dir）。
pub fn step_separate_after(
    sc: impl StepCtxView,
    deps: SeparateAfterDeps,
) -> anyhow::Result<SeparateAfterOutput> {
    let input = sc.ctx().input();
    let video_dir = input.video_dir().to_string();
    tracing::info!(target: "separate", "start");

    set_step_anyhow(
        &video_dir,
        "separate_after",
        StepPatch {
            last_message: Some("Mixing BGM & sidechain...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let out_dir = separate_after_dir(&video_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("创建 separate_after 目录失败: {e}"))?;

    // stems 来自 separate 步骤的输出目录(见 `SeparateAfterDeps`)。
    let sep_dir = deps.stems_dir.as_path().to_path_buf();
    let stems = [
        ("drums", sep_dir.join("target_0_drums.wav")),
        ("bass", sep_dir.join("target_1_bass.wav")),
        ("other", sep_dir.join("target_2_other.wav")),
        ("vocals", deps.vocals_path.as_path().to_path_buf()),
    ];
    let vocals_path = stems[3].1.clone();
    let bgm_dst = out_dir.join("target_bgm.wav");
    // 产出路径走 separate_after 的 newtype, 拼法单一来源 (消费者 AsrDeps 同款)。
    let mixed_dst = Target3VocalsMixedPath::new(&video_dir);

    // 1. 由 stems 重建 target_bgm.wav (修复 amix normalize bug)
    let all_stems_exist = stems[..3].iter().all(|(_, p)| p.exists());
    if all_stems_exist {
        tracing::info!(target: "separate", "Generating target_bgm.wav (amix normalize=0)...");
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
        tracing::info!(target: "separate", "target_bgm.wav exists, reusing (stems not all found)");
    } else {
        tracing::info!(target: "separate", "No stems or BGM found, skipping BGM generation");
    }

    // 2. 按本步骤专属混音参数生成 sidechain 混音人声
    let args = &input.steps.separate_after;
    let use_separated = input.steps.asr.use_separated; // 与 asr 共用的音源决策
    let mix_mode = args.mix_mode;
    let reduce_bgm = args.reduce_bgm;
    let sc = args.sidechain_compress.clone();
    let use_gate = args.use_gate;

    if use_separated && !matches!(mix_mode, MixMode::Vocals) {
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
            MixMode::RawSum => {
                tracing::info!(target: "separate",
                    "[SeparateAfter] raw-sum: mixing vocals + BGM at {reduce_bgm}dB..."
                );
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
                    mixed_dst.as_path().to_string_lossy().into_owned(),
                ])?;
            }
            MixMode::Sidechain => {
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
                tracing::info!(target: "separate",
                    "[SeparateAfter] sidechain: {sc_params}, bgmReduce={reduce_bgm}dB"
                );
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
                    mixed_dst.as_path().to_string_lossy().into_owned(),
                ])?;
            }
            MixMode::Vocals => unreachable!(),
        }
    }

    // 3. 可选 silence gate
    if use_gate && mixed_dst.as_path().exists() {
        let gated_path = GatedVocalsPath::new(&video_dir);
        tracing::info!(target: "separate", "Applying silence gate...");
        ffmpeg(&[
            "-i".into(),
            mixed_dst.as_path().to_string_lossy().into_owned(),
            "-af".into(),
            "agate=threshold=0.02:ratio=20:attack=10:release=100".into(),
            gated_path.as_path().to_string_lossy().into_owned(),
        ])?;
    }

    set_step_anyhow(
        &video_dir,
        "separate_after",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Done".into()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "separate", "done");
    // 以文件是否实际存在为准，精确反映本次产出了哪些（bgm 可能沿用旧文件）。
    let mixed_path = Target3VocalsMixedPath::new(&video_dir);
    let gated_path = GatedVocalsPath::new(&video_dir);
    Ok(SeparateAfterOutput {
        bgm_path: bgm_dst.exists().then_some(bgm_dst),
        mixed_path: mixed_path.as_path().exists().then(|| mixed_path.as_path().to_path_buf()),
        gated_path: gated_path.as_path().exists().then(|| gated_path.as_path().to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::utils::separate_dir;
    use serde_json::json;

    fn ffmpeg_available() -> bool {
        std::process::Command::new(crate::steps::utils::ffmpeg_bin())
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

        let input = crate::state::write_fixture(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {
                    "asr": {"use_separated": true},
                    "separate_after": {"mix_mode": "sidechain", "reduce_bgm": -12, "use_gate": true}
                }},
                "output_mode": "dub"
            }),
        );

        let wfi = crate::input::WorkflowInput::try_from(input).unwrap();
        let deps = SeparateAfterDeps::from_input(&wfi);
        let res = step_separate_after(
            crate::workflows::step_ctx::MyStepCtx::new("separate_after", wfi),
            deps,
        );
        assert!(res.is_ok(), "separate_after 失败: {:?}", res.err());

        assert!(
            separate_after_dir(&dir).join("target_bgm.wav").exists(),
            "target_bgm.wav 应生成"
        );
        assert!(
            Target3VocalsMixedPath::new(&dir).as_path().exists(),
            "target_3_vocals_mixed.wav 应生成"
        );
        assert!(
            GatedVocalsPath::new(&dir).as_path().exists(),
            "useGate=true 时 target_3_vocals_gated.wav 应生成"
        );

        let st = crate::state::read_step_state(&dir, "separate_after").unwrap();
        assert_eq!(st.status, StepStatus::Success);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
