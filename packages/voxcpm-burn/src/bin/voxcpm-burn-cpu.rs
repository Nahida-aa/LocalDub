//! NdArray CPU 后端。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    voxcpm_burn::init_logging();
    let cli = voxcpm_burn::Cli::parse();
    voxcpm_burn::run::<burn::backend::NdArray<f32>>(cli, Default::default())
}
