//! Minimal CLI to exercise the `voxlab` cloud TTS backend.
//!
//! Usage:
//!   voxlab --ref <reference.wav> --text "..." [--out out.wav] [--api-url https://...] [--prompt "ref transcript"] [--cfg 2.0]

use std::path::PathBuf;
use voxlab::{VoxCPMCloud, VoxCPMCloudConfig, write_wav};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut ref_path = None;
    let mut text = "好的，我们现在在大象面前。".to_string();
    let mut out = PathBuf::from("packages/voxlab/tmp/voxlab-cloud-out.wav");
    let mut api_url = None;
    let mut prompt = None;
    let mut cfg = 2.0;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ref" => {
                ref_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--text" => {
                text = args[i + 1].clone();
                i += 2;
            }
            "--out" => {
                out = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--api-url" => {
                api_url = Some(args[i + 1].clone());
                i += 2;
            }
            "--prompt" => {
                prompt = Some(args[i + 1].clone());
                i += 2;
            }
            "--cfg" => {
                cfg = args[i + 1].parse().unwrap_or(2.0);
                i += 2;
            }
            other => {
                eprintln!("ignoring unknown arg: {other}");
                i += 1;
            }
        }
    }

    let ref_path = ref_path.ok_or_else(|| anyhow::anyhow!("missing --ref <reference.wav>"))?;

    let cloud = VoxCPMCloud::new(VoxCPMCloudConfig {
        api_url,
        control_instruction: None,
    });

    println!("[voxlab] generating via cloud backend...");
    let result = cloud.generate(&text, &ref_path, prompt.as_deref(), cfg)?;

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    write_wav(&result.samples, result.sample_rate, &out.to_string_lossy())?;

    let dur = result.samples.len() as f64 / result.sample_rate as f64;
    println!(
        "[voxlab] wrote {:.2}s @ {}Hz to {} (gen {:.2}s)",
        dur,
        result.sample_rate,
        out.display(),
        result.gen_time_sec
    );
    Ok(())
}
