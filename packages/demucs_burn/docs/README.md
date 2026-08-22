# Demucs 经验文档

| 文档 | 内容 |
|---|---|
| [cpu-fallback.md](./cpu-fallback.md) | GPU hang 根因（CrossTransformer + Wiener）→ CPU fallback 结论 |
| [pytorch-cpu-optimization.md](./pytorch-cpu-optimization.md) | PyTorch CPU 路径优化方案分析（历史记录，部分路径已过时） |
| [conv-patch.md](./conv-patch.md) | GEMM 卷积 patch — GPU hang 规避参考，未启用 |
| [models.md](./models.md) | 模型权重来源（PyTorch `.th` / ONNX bag / burn safetensors） |

## 后端基准

各后端 RTF/CER 实测数据与脚本同目录：[`packages/benchmark/separate/`](../../../benchmark/separate/)。

关键结论（htdemucs_ft，Ryzen 7 H 255 + Radeon 780M）：

| 后端 | RTF | 备注 |
|---|---|---|
| burn tch (libtorch MKL) | **1.26–1.66** | 当前生产默认，需 `LD_LIBRARY_PATH` |
| GGML (OpenBLAS+OMP) | ~1.36–1.41 | shift=1 硬编码；mt=4 + `OMP_NUM_THREADS=2` 最优 |
| ORT bag (vocals-only) | ~1.4–2.2 | per-stem fp16 模型 |
| PyTorch CPU shifts=3 | ~7.9–11.9 | 质量最好 (CER 7.18%) 但最慢 |
| burn wgpu (RADV Vulkan) | ~2.3–2.8 | 需 warmup；autotune 会触发驱动 hang |

## 已知问题速查

- 整网 GPU 推理 → GPU Hang 黑屏（gfx1103 MES firmware 0x83）；CPU 是唯一稳定路径
- 分离后 vocals 低噪 → whisper 幻觉循环；sidechain compression (-12dB BGM 填充) 可消除
- GGML 输出 32-bit float WAV 使 whisper.cpp CER 劣化到 14.72%，须转 16-bit
