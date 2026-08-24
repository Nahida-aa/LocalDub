//! vulkan 后端（burn/vulkan feature, bf16 精度）。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    voxcpm_burn::init_logging();
    let cli = voxcpm_burn::Cli::parse();
    voxcpm_burn::run::<burn::backend::Vulkan<half::bf16, i32>>(cli, Default::default())
}
