use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use demucs_core::listener::{ForwardEvent, ForwardListener};
use demucs_core::model::metadata::{
    ALL_MODELS, HTDEMUCS_6S_ID, HTDEMUCS_FT_ID, HTDEMUCS_ID, ModelInfo,
};
use demucs_core::provider::ModelProvider;
use demucs_core::provider::fs::FsProvider;
use demucs_core::{Demucs, ModelOptions};

#[cfg(feature = "cubecl-cpu")]
type B = burn::backend::Cpu;

#[cfg(feature = "cubecl-wgpu")]
type B = burn::backend::wgpu::Wgpu;

#[cfg(feature = "cubecl-rocm")]
type B = burn::backend::Rocm;

#[cfg(feature = "cubecl-cuda")]
type B = burn::backend::Cuda;

#[cfg(feature = "tch")]
type B = burn::backend::LibTorch;

#[derive(Parser)]
#[command(name = "demucs-burn")]
struct Cli {
    /// Benchmark mode: load model and print timing, then exit
    #[arg(long)]
    benchmark_load: bool,

    /// Input WAV file (for separation or load test)
    input: Option<PathBuf>,

    /// Output directory for stems
    output: Option<PathBuf>,

    /// Model variant
    #[arg(short, long, default_value = "htdemucs_ft")]
    model: String,

    /// Wgpu tasks_max (CPU threads for command recording). Default 128.
    #[arg(long, default_value = "128")]
    tasks_max: u32,

    /// Run warmup inference to pre-compile GPU shaders.
    #[arg(long)]
    warmup: bool,

    /// Benchmark rounds: run separation N times in one process (default 1).
    #[arg(long, default_value = "1")]
    benchmark_rounds: u32,
}

fn resolve_model_info(model_id: &str) -> Result<&'static ModelInfo> {
    ALL_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .copied()
        .with_context(|| format!("Unknown model: {}", model_id))
}

fn model_options(info: &ModelInfo) -> ModelOptions {
    match info.id {
        HTDEMUCS_ID => ModelOptions::FourStem,
        HTDEMUCS_6S_ID => ModelOptions::SixStem,
        HTDEMUCS_FT_ID => ModelOptions::FineTuned(info.stems.to_vec()),
        _ => ModelOptions::FourStem,
    }
}

struct BenchListener;

impl ForwardListener for BenchListener {
    fn on_event(&mut self, event: ForwardEvent) {
        match event {
            ForwardEvent::ChunkStarted { index, total } => {
                let pct = (index as f64 / total as f64 * 100.0) as u32;
                println!("({}%)", pct.min(99));
            }
            ForwardEvent::ChunkDone { index, total } => {
                let pct = ((index + 1) as f64 / total as f64 * 100.0) as u32;
                println!("({}%)", pct.min(100));
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    run()
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let info = resolve_model_info(&cli.model)?;
    let opts = model_options(info);

    let provider = FsProvider::with_dir(config_rs::path::models::demucs_model_dir());
    let bytes = if provider.is_cached(info) {
        eprintln!("Loading cached model: {}", info.id);
        provider
            .load_cached(info)
            .context("Failed to load cached model")?
    } else {
        anyhow::bail!(
            "Model '{}' not cached. Run demucs-cli first to download it.",
            info.id
        );
    };

    #[cfg(feature = "cubecl-wgpu")]
    let device = {
        use burn::backend::wgpu::{RuntimeOptions, graphics::AutoGraphicsApi, init_setup};
        let d = Default::default();
        let options = RuntimeOptions {
            tasks_max: cli.tasks_max as usize,
            ..Default::default()
        };
        init_setup::<AutoGraphicsApi>(&d, options);
        d
    };

    #[cfg(not(feature = "cubecl-wgpu"))]
    let device = Default::default();

    let load_start = Instant::now();
    let model =
        Demucs::<B>::from_bytes(opts, &bytes, device).context("Failed to load model weights")?;
    let load_time = load_start.elapsed();
    eprintln!("Model loaded in {:.3}s", load_time.as_secs_f64());
    println!("Benchmark-Load-Time: {:.3}", load_time.as_secs_f64());

    if cli.warmup {
        #[cfg(feature = "cubecl-wgpu")]
        {
            eprintln!("Pre-compiling GPU shaders (first run only)...");
            let warmup_start = Instant::now();
            pollster::block_on(model.warmup());
            let t = warmup_start.elapsed();
            eprintln!("Warmup done in {:.1}s", t.as_secs_f64());
            println!("Benchmark-Warmup-Time: {:.3}", t.as_secs_f64());
        }
        #[cfg(not(feature = "cubecl-wgpu"))]
        eprintln!("Skipping GPU warmup (not wgpu backend)");
    }

    if cli.benchmark_load {
        return Ok(());
    }

    let input = cli.input.context("Input file required")?;
    let out_dir = cli.output.context("Output directory required")?;

    eprintln!("Reading {}", input.display());
    let (left, right, sample_rate) = read_wav(&input)?;
    let duration_secs = left.len() as f64 / sample_rate as f64;
    eprintln!(
        "  {} samples, {:.1}s, {} Hz, stereo",
        left.len(),
        duration_secs,
        sample_rate
    );

    for round in 1..=cli.benchmark_rounds {
        eprintln!("--- Round {}/{} ---", round, cli.benchmark_rounds);
        let sep_start = Instant::now();
        let stems = pollster::block_on(model.separate_with_listener(
            &left,
            &right,
            sample_rate,
            &mut BenchListener,
        ))?;
        let sep_time = sep_start.elapsed();
        eprintln!("Generate (round {}): {:.3}s", round, sep_time.as_secs_f64());
        println!(
            "Benchmark-Gen-Time-Round{}: {:.3}",
            round,
            sep_time.as_secs_f64()
        );

        if round == cli.benchmark_rounds {
            std::fs::create_dir_all(&out_dir)?;
            for (i, stem) in stems.iter().enumerate() {
                let filename = format!("target_{}_{}.wav", i, stem.id.as_str());
                let path = out_dir.join(&filename);
                write_wav(&path, &stem.left, &stem.right, sample_rate)?;
                eprintln!("  Wrote {}", path.display());
            }
        }
    }

    println!("(100%)");
    eprintln!("Done!");
    Ok(())
}

fn read_wav(path: &PathBuf) -> Result<(Vec<f32>, Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("Failed to open WAV: {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels > 2 {
        anyhow::bail!("Expected mono or stereo, got {} channels", spec.channels);
    }
    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let max = (1u32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max)
                .collect()
        }
    };

    if spec.channels == 1 {
        Ok((samples.clone(), samples, sample_rate))
    } else {
        let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
        let right: Vec<f32> = samples.iter().skip(1).step_by(2).copied().collect();
        Ok((left, right, sample_rate))
    }
}

fn write_wav(path: &std::path::Path, left: &[f32], right: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for (&l, &r) in left.iter().zip(right.iter()) {
        writer.write_sample(l)?;
        writer.write_sample(r)?;
    }
    writer.finalize()?;
    Ok(())
}
