# Model device assignment

| Model | Runtime | Device | Reason |
|-------|---------|--------|--------|
| Demucs | Python (PyTorch) | CPU | GPU hang (CrossTransformer + Wiener filter) |
| VoxCPM (TTS) | Python (PyTorch) | CPU | GPU: model loads but any forward pass → segfault |
| VoxCPM (TTS) | vLLM-Omni (ROCm) | ❌ HIP ABI 不兼容 (PyTorch 2.10 vs 2.12) + GPU Hang |
| VoxCPM (TTS) | ORT+MIGraphX | GPU (ROCm) | ✅ 可用，**Split strategy**: Prefill/VAE → CPU, Decode → GPU |
| VoxCPM (TTS) | TypeScript (ONNX) | CPU / webgpu | ❌ 废弃，Bun 多版本冲突 + `sharp` 依赖 + OOM |
| CosyVoice3 (TTS) | Python (ONNX) | CPU | 无 CUDA EP，编译 ORT+MIGraphX 中（MIOpen conv solver hang）|
| Whisper (ASR) | whisper.cpp | Vulkan (RADV) | ✅ **首选**，RTF ~0.09，编译 `-DGGML_VULKAN=ON` |
| Whisper (ASR) | Python (faster-whisper / CTranslate2) | CPU | ⚠️ 暂停止支持 |
| Whisper (ASR) | Python (PyTorch) | CPU | ⚠️ 暂停止支持 |
| Whisper (ASR) | Python (faster-whisper) | GPU | ❌ CTranslate2 CUDA 依赖 libcuda.so+libcudart.so，ROCm 系统无 |
| Whisper (ASR) | whisper.cpp HIPBLAS | GPU | ❌ MES 0x83 `REMOVE_QUEUE` hang，长音频必触发，已废弃 |
| Translation | Python (OpenAI) | N/A | Remote API |

## ORT+MIGraphX hybrid strategy

Build ORT 1.25.0 from source with `--use_migraphx`. Uses `MIGRAPHX_DISABLE_MIOPEN_FUSION=1` env var to prevent MIOpen conv solver hang on gfx1103.

| Submodel | Device | EP | Time (first run) | Notes |
|----------|--------|----|-------------------|-------|
| VAE Encoder | CPU | CPU | 0.606s | MIOpen GemmFwdRest hang |
| Prefill (2B) | CPU | CPU | 28.9s | MIGraphX compilation OOM |
| Decode Step | GPU | MIGraphX | 553.7ms/step (25 steps) | KV cache growth → per-step compilation |
| VAE Decoder | CPU | CPU | 0.453s | MIOpen GemmFwdRest hang |

**Pitfalls**:
- Decode step triggers `GemmFwdRest` solver via MIGraphX fusion passes → must set `MIGRAPHX_DISABLE_MIOPEN_FUSION=1`
- VAE models use Conv/ConvTranspose → MIOpen solver evaluation unavoidable → CPU EP required
- Prefill 2B ONNX (8.4GB) + MIGraphX compilation workspace → OOM on 28GB system
- Dynamic KV cache shape triggers recompilation each step

**Future optimization**: Pad KV cache to max_len to compile once; MIGRAPHX_CACHE_DIR for persistence.

## Python backend (`backend/app/`)

所有模型推理仅在 pipeline runner 内部按顺序执行，没有单独的 HTTP 端点暴露。

## TypeScript 端 (`packages/api/src/ml/`)

当前只有 VoxCPM 有 TS 实现（`voxcpm/` 目录）：
- `voxcpm.ts` — 类定义 + ONNX 推理管线
- `load.ts` — 模型文件状态检查
- `download.ts` — 从 HuggingFace 下载 ONNX 模型
- `device-info.ts` + `device-route.ts` — 设备信息 API
