#![recursion_limit = "256"]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use voxcpm_rs::{GenerateOptions, VoxCPM, audio};

#[cfg(all(feature = "vulkan", not(feature = "wgpu")))]
type B = burn::backend::Vulkan<half::bf16, i32>;
#[cfg(all(feature = "wgpu", not(feature = "vulkan")))]
type B = burn::backend::Wgpu<f32, i32>;
#[cfg(feature = "cpu")]
type B = burn::backend::NdArray<f32>;
#[cfg(feature = "tch")]
type B = burn::backend::LibTorch<half::bf16>;

#[derive(Parser)]
#[command(name = "voxcpm-burn")]
struct Cli {
    #[arg(long)]
    benchmark_load: bool,

    text: Option<String>,

    output: Option<PathBuf>,

    #[arg(long, default_value = "")]
    model_dir: String,

    #[arg(long, default_value_t = 10)]
    timesteps: usize,

    #[arg(long, default_value_t = 2.0)]
    cfg: f32,

    #[arg(long, default_value_t = 500)]
    max_len: usize,

    #[arg(long)]
    warmup: bool,

    /// Parallel segment generation (batch N sentences). GPU sweet spot ~8.
    #[arg(long)]
    parallel_segments: Option<usize>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("info,wgpu_hal=error,wgpu_core=error,naga=error,cubecl_wgpu=warn"),
    )
    .init();

    let cli = Cli::parse();

    let model_dir = if cli.model_dir.is_empty() {
        config_rs::path::models::voxcpm_model_dir()
    } else {
        PathBuf::from(&cli.model_dir)
    };

    eprintln!("Loading model from {}", model_dir.display());

    let device = Default::default();
    let load_start = Instant::now();
    let model: VoxCPM<B> =
        VoxCPM::from_local(&model_dir, &device).context("Failed to load model")?;
    let load_time = load_start.elapsed();
    eprintln!("Model loaded in {:.3}s", load_time.as_secs_f64());
    println!("Benchmark-Load-Time: {:.3}", load_time.as_secs_f64());

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
    audio::write_wav(&out_path, &wav, sr).context("Failed to write WAV")?;
    eprintln!("Done!");
    Ok(())
}
