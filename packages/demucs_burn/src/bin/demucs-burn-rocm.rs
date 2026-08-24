//! ROCm (HIP) 后端。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = demucs_burn::Cli::parse();
    demucs_burn::run::<burn::backend::Rocm>(cli, Default::default())
}
