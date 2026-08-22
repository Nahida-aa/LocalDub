//! vulkan 后端（burn/vulkan feature, 运行时仍是 wgpu runtime）。
use burn::backend::wgpu::{RuntimeOptions, graphics::AutoGraphicsApi, init_setup};
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
