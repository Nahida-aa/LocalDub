//! libtorch 后端（bf16 精度; 运行需注入 LD_LIBRARY_PATH 指向 libtorch lib 目录）。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    voxcpm_burn::init_logging();
    let cli = voxcpm_burn::Cli::parse();
    voxcpm_burn::run::<burn::backend::LibTorch<half::bf16>>(cli, Default::default())
}
