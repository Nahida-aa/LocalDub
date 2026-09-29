//! 引擎/接线测试共用的 fixture（仅 cfg(test) 编译）。

use serde_json::json;
use subtitle::SubtitleSegment;

use crate::state::{read_ctx, read_ctx_from_value, write_ctx, StepStatus};

pub fn temp_dir(name: &str) -> String {
    let dir = std::env::temp_dir()
        .join(format!("ld_engine_{name}_{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// 构造写好 state.json 的 subtitle 临时 workflow 目录 (只触发 separate / separate_after).
pub fn setup_subtitle(dir: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let input = json!({
        "steps": {
            "separate": {"always": false},
            "asr": {"enabled": false},
            "asr_fix": {"enabled": false},
            "translate": {"enabled": false},
            "mix_video": {"enabled": false}
        }
    });
    let mut ctx = read_ctx_from_value(json!({
        "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                     "status":"running","created_at":"2024-01-01T00:00:00Z"},
        "input": input,
        "output_mode": "subtitle"
    }))
    .unwrap();
    ctx.workflow.video_dir = dir.to_string();
    ctx.workflow.id = "t".to_string();
    ctx.output_mode = "subtitle".to_string();
    // 模拟 import 已跑过：subtitle 序列现恒含 split_audio，无 vocals stem 时它
    // 回退到 audio_source.wav，缺失即报错（文件本身不必存在，probe 失败按 0 处理）。
    ctx.audio_source_path = Some(format!("{dir}/audio_source.wav"));
    write_ctx(dir, &ctx).unwrap();
    // split_audio 会读 translate 产物；本 stub 禁用了 asr/asr_fix，translate 直通
    // 无内容可搬会写出空产物，split_audio 随即 fail-fast。故给 translate 的
    // **输入**（subtitleSource=asr → asr_fix/srt.json）造假数据，直通原样带出。
    let p = std::path::Path::new(dir).join("asr_fix").join("srt.json");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::steps::utils::write_srt_json(
        &p,
        &[
            SubtitleSegment {
                text: "一".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            SubtitleSegment {
                text: "二".into(),
                start_ms: 1000,
                end_ms: 2000,
            },
        ],
    )
    .unwrap();
}

pub fn status_of(dir: &str, step: &str) -> StepStatus {
    let ctx = read_ctx(dir).unwrap();
    ctx.steps
        .unwrap()
        .iter()
        .find(|s| s.name == step)
        .unwrap()
        .status
}
