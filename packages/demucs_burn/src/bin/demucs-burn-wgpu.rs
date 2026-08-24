//! wgpu 后端（Linux 上经 RADV 走 Vulkan）。
use burn::backend::wgpu::{graphics::AutoGraphicsApi, init_setup, RuntimeOptions};
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = demucs_burn::Cli::parse();
    let device = Default::default();
    init_setup::<AutoGraphicsApi>(
        &device,
        RuntimeOptions {
            tasks_max: cli.tasks_max as usize,
            ..Default::default()
        },
    );
    demucs_burn::run::<burn::backend::wgpu::Wgpu>(cli, device)
}
