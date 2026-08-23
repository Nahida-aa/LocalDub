//! CubeCL CPU 后端（MLIR JIT，仅实验用途）。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = demucs_burn::Cli::parse();
    demucs_burn::run::<burn::backend::Cpu>(cli, Default::default())
}
