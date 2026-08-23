# conv_patch.py (`backend/app/adapters/conv_patch.py`)

Bypasses MIOpen by replacing `F.conv1d` / `F.conv2d` / `F.conv_transpose1d` / `F.conv_transpose2d` with GEMM-based implementations via `F.unfold` + `torch.matmul`. Verified correct on CPU against native PyTorch for all stride/padding/dilation/groups combos.

Currently **not applied anywhere** — Demucs runs on CPU with native PyTorch conv. The patch is preserved as a reference for future optimization:

- Replace `F.unfold` + `matmul` with a custom kernel (CUDA, ROCm, or pure Rust GEMM via PyO3 FFI).
- Same API surface: just swap `_conv1d_gemm` → Rust conv1d, call `apply_patch()` before Demucs import.
- Could also rewrite CrossTransformer attention (self-attn + cross-attn + FFN) in Rust to eliminate the GPU hang source entirely.
