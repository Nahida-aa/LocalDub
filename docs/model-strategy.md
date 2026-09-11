# Model Strategy: VoxCPM2 vs CosyVoice3

## Comparison

| Aspect | VoxCPM2 | CosyVoice3 |
|--------|---------|-------------|
| **Architecture** | DiT (Diffusion Transformer) | LLM + Flow Matching + HiFT |
| **Backbone** | MiniCPM-4 (2B) | Qwen2 (~0.5B) |
| **Pipeline steps** | LocEnc 12L → TSLM 28L → RALM 8L → LocDiT 12L | LLM backbone → Flow decoder → HiFT vocoder |
| **ONNX models** | 4 (prefill, decode, vae_enc, vae_dec) | 14 (text_emb, llm_initial, llm_decode, flow, hift...) |
| **Output SR** | 48 kHz (built-in super-resolution) | 24 kHz |
| **RTF CPU** | **~7-10×** | **~18-44×** |
| **Integration** | ✅ Active in production pipeline | ❌ Not integrated |
| **PyTorch GPU** | ❌ Segfault on RDNA 3 | N/A (ONNX only) |
| **ONNX WebGPU** | ⚠️ Works short, OOM medium+ | N/A |
| **ONNX CUDA** | ❌ Missing cuDNN 9 | ❌ Missing cuDNN 9 |
| **llama.cpp potential** | Low (DiT bottleneck, not LLM) | High (LLM is 56-63% time, Qwen2-based) |
| **Code complexity** | Simpler (single model) | Complex (3-step pipeline) |

## Performance (CPU, same English texts)

| Text | VoxCPM2 PyTorch | VoxCPM2 TS ONNX | CosyVoice3 ONNX |
|------|:---------------:|:---------------:|:---------------:|
| Short (19ch) | 14.3s / RTF 9.9 | 49.4s / RTF 7.3 | 72.1s / RTF 36.8 |
| Medium (90ch) | 58.2s / RTF 8.9 | 161.5s / RTF 7.3 | 142.8s / RTF 25.9 |
| Long (288ch) | 150.2s / RTF 9.0 | 307.5s / RTF 7.1 | 353.5s / RTF 17.7 |

## Decision Factors

### Keep VoxCPM2 if:
- RTF ~9× is acceptable (current state, proven quality)
- Can't or won't fix GPU
- Simpler maintenance wins over quality

### Switch to CosyVoice3 if:
- Voice cloning quality is noticeably better (subjective)
- Can accelerate LLM via llama.cpp or CUDA EP
- Willing to invest in integration + pipeline refactoring

### Recommended path:
CuDNN 9 installation is the **highest-leverage** action — it unlocks CUDA EP for **both** models, benefits all ONNX pipelines, and removes the biggest hardware bottleneck.
