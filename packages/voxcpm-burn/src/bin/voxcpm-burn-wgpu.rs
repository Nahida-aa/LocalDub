//! wgpu 后端（Linux 上经 RADV 走 Vulkan）。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    voxcpm_burn::init_logging();
    let cli = voxcpm_burn::Cli::parse();
    voxcpm_burn::run::<burn::backend::wgpu::Wgpu<f32, i32>>(cli, Default::default())
}
