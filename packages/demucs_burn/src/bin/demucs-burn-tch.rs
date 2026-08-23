//! libtorch CPU 后端（运行需注入 LD_LIBRARY_PATH 指向 libtorch lib 目录）。
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = demucs_burn::Cli::parse();
    demucs_burn::run::<burn::backend::LibTorch>(cli, Default::default())
}
