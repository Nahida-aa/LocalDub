//! voxcpm-burn 公共逻辑。各后端二进制（`src/bin/voxcpm-burn-*`）是薄壳：
//! 选定后端类型后调用 [`run`]。
#![recursion_limit = "256"]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use burn::prelude::Backend;
use clap::Parser;
use voxcpm_rs::{GenerateOptions, VoxCPM};

#[derive(Parser)]
#[command(name = "voxcpm-burn")]
pub struct Cli {
    #[arg(long)]
    pub benchmark_load: bool,

    pub text: Option<String>,

    pub output: Option<PathBuf>,

    #[arg(long, default_value = "")]
    pub model_dir: String,

    #[arg(long, default_value_t = 10)]
    pub timesteps: usize,

    #[arg(long, default_value_t = 2.0)]
    pub cfg: f32,

    #[arg(long, default_value_t = 500)]
    pub max_len: usize,

    #[arg(long)]
    pub warmup: bool,

    /// Parallel segment generation (batch N sentences). GPU sweet spot ~8.
    #[arg(long)]
    pub parallel_segments: Option<usize>,

    /// Reference audio for voice cloning (zero-shot). Pass a WAV/FLAC/MP3 path.
    #[arg(long, default_value = "")]
    pub ref_audio: String,
}

/// 主流程。stdout 约定（基准脚本依赖，勿改）：`Benchmark-Load-Time` / `Benchmark-Warmup-Time` / `Benchmark-Gen-Time`。
pub fn run<B: Backend>(cli: Cli, device: B::Device) -> Result<()> {
    let model_dir = if cli.model_dir.is_empty() {
        config_rs::path::models::voxcpm_model_dir()
    } else {
        PathBuf::from(&cli.model_dir)
    };

    eprintln!("Loading model from {}", model_dir.display());

    let load_start = Instant::now();
    let model: VoxCPM<B> =
        VoxCPM::from_local(&model_dir, &device).context("Failed to load model")?;
    let load_time = load_start.elapsed();
    eprintln!("Model loaded in {:.3}s", load_time.as_secs_f64());
    println!("Benchmark-Load-Time: {:.3}", load_time.as_secs_f64());

    // 仅 wgpu/vulkan 是 GPU-shader 后端; 与旧互斥 cfg 链的 any(wgpu|vulkan) 等价。
    if cli.warmup {
        #[cfg(any(feature = "wgpu", feature = "vulkan"))]
        {
            eprintln!("Pre-compiling GPU shaders (first run only)...");
            let warmup_start = Instant::now();
            let opts = GenerateOptions::builder()
                .timesteps(2)
                .cfg(1.0)
                .max_len(10)
                .build();
            let _ = model.generate("warmup", opts);
            let t = warmup_start.elapsed();
            eprintln!("Warmup done in {:.1}s", t.as_secs_f64());
            println!("Benchmark-Warmup-Time: {:.3}", t.as_secs_f64());
        }
        #[cfg(not(any(feature = "wgpu", feature = "vulkan")))]
        eprintln!("Skipping GPU warmup (not GPU backend)");
    }

    if cli.benchmark_load {
        return Ok(());
    }

    let text = cli.text.context("Text argument required")?;
    let out_path = cli
        .output
        .unwrap_or_else(|| PathBuf::from("/tmp/voxcpm_out.wav"));

    let opts = {
        let mut b = GenerateOptions::builder()
            .timesteps(cli.timesteps)
            .cfg(cli.cfg)
            .max_len(cli.max_len);
        if let Some(n) = cli.parallel_segments {
            b = b.parallel_segments(n);
        }
        if !cli.ref_audio.is_empty() {
            b = b.prompt(voxcpm_rs::Prompt::Reference {
                audio: voxcpm_rs::PromptAudio::File(std::path::PathBuf::from(
                    cli.ref_audio.clone(),
                )),
            });
        }
        b.build()
    };

    eprintln!("Synthesizing: {:?}", text);
    let gen_start = Instant::now();
    let wav = model.generate(&text, opts).context("Generation failed")?;
    let gen_time = gen_start.elapsed();
    let sr = model.sample_rate();
    let audio_sec = wav.len() as f64 / sr as f64;
    eprintln!(
        "Got {} samples @ {} Hz ({:.2}s audio) in {:.3}s (RTF={:.2})",
        wav.len(),
        sr,
        audio_sec,
        gen_time.as_secs_f64(),
        gen_time.as_secs_f64() / audio_sec
    );
    println!("Benchmark-Gen-Time: {:.3}", gen_time.as_secs_f64());

    eprintln!("Writing {}", out_path.display());
    voxcpm_rs::audio::write_wav(&out_path, &wav, sr).context("Failed to write WAV")?;
    eprintln!("Done!");
    Ok(())
}

pub fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
        "info,wgpu_hal=error,wgpu_core=error,naga=error,cubecl_wgpu=warn",
    ))
    .init();
}
