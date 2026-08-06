# ocr-lab：OCR 核心源码出处（git 源）

OCR 核心（`subtitle-finder` / `subtitle-ocr` / `rapidocr-ort` / `geometry`）是从本仓库拆出的独立
workspace，托管在 ocrlab，通过 **crates.io git 源 + rev** 引入，cargo 自动拉取/解析：

- 仓库：`https://github.com/Nahida-aa/ocr-lab.git`（本地开发复制在 `/home/aa/repos/ai_ls/ocr-lab`）
- 声明位置：LocalDub 根 `Cargo.toml` `[workspace.dependencies]`，pin `rev = "202310f"`
- 消费方：`packages/sf-ocr`（关键帧策略：subtitle-finder 提关键帧 → subtitle-ocr 识别）
- 升级：改 ocr-lab 代码 → 在 ocr-lab push → 更新此根 Cargo.toml 的 `rev`；不跑 git submodule

**版本以 ocr-lab 为准**：`ort 2.0.0-rc.13`、`ndarray 0.17`、`opencv 0.100`（subtitle-finder 需）。

## 为什么

- git 源让 cargo 自动拉取，消费方（他人 clone 后 `cargo build`）无需手动 git 克隆；ocr-lab
  内部的 `[workspace]`/`[patch]`/`workspace=true` 继承在它自身解析，LocalDub 零镜像。

## 现状与继承问题（2026-08）

- `packages/sf-ocr` 用 ocr-lab 的 `opencv 0.100.1` 等能编译通过（`cargo check -p sf-ocr` ✓）。
- `packages/subtitle-rust`（旧 OCR 管道）与其 pin 的 `opencv 0.98.2` 已**无法在现 rustc 下编译**
  （`MatShape`/`MatStep` 生命周期错误）——属预期：它依赖旧 OCR 依赖，接进来后退役，
  其 `--engine rust` 基准将被 sf 路径替代。
- Cargo.lock 中 opencv 并存 0.98.2（subtitle-rust）与 0.100.1（ocr-lab），是分裂的临时状态；
  退役 `subtitle-rust` 后 0.98 一并消失。

## 下一步（集成阶段，未做）

- 退役 `packages/subtitle-rust` 及其 `--engine rust`/`RUST_BIN` 基准引用
- 将 sf 关键帧策略接进 `packages/benchmark/ocr/compute/benchmark-ocr-video.ts`
- 完成后上面「现状」段的临时分裂状态即可清除