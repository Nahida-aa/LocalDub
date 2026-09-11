# Application Architecture

## Overview

YouDub-webui is a **video dubbing** application with voice cloning. Pipeline:

```
Input (YouTube/Bilibili/Local)
  → [0] Download (yt-dlp)
  → [1] Separate (Demucs) → vocals + bgm
  → [2] ASR (Whisper) → text + timestamps
  → [3] Fix sentences
  → [4] Translate (OpenAI API)
  → [5] Split audio → segments
  → [6] TTS (VoxCPM) → voice-cloned segments
  → [7] Merge audio → time-stretched dubbing
  → [8] Merge video → final MP4 + SRT
```

## Tech Stack

| Layer      | Stack                                         | Port |
| ---------- | --------------------------------------------- | ---- |
| Frontend   | Next.js 15, React 19, Tailwind CSS, shadcn/ui | 3000 |
| Backend    | Python FastAPI, uvicorn, SQLite               | 8000 |
| API (exp.) | Hono/Bun TypeScript                           | 9007 |

## Backend Structure

```
backend/app/
├── main.py           # FastAPI entry + routes
├── pipeline.py       # PipelineRunner: orchestrates 9 steps
├── steps.py         # StepSpec definitions
├── worker.py         # Single-thread FIFO worker
├── config.py         # Paths & env vars
├── database.py       # SQLite (tasks, task_steps, settings)
├── devices.py        # Per-component device selection
├── adapters/         # Model adapters
│   ├── demucs.py         # Voice separation (CPU)
│   ├── whisper_asr.py    # ASR (GPU cuda)
│   ├── voxcpm.py         # TTS / Voice cloning (CPU)
│   ├── openai_translate.py  # Translation (remote API)
│   ├── audio.py          # Audio split/stretch
│   ├── ffmpeg.py         # Video merge + subtitles
│   ├── asr_sentence_fixer.py
│   └── conv_patch.py     # GEMM conv shim (unused)
```

## TTS / Voice Cloning

The TTS step (step 6) uses **VoxCPM2** (OpenBMB/VoxCPM2):

- **Runtime**: Python PyTorch (`voxcpm` PyPI package)
- **Device**: CPU (GPU forward pass segfaults on RDNA 3)
- **Pipeline**: LocEnc → TSLM MiniCPM-4 (2B) → RALM → LocDiT → VAE Decode
- **Output**: 48kHz mono WAV
- **Reference**: For each segment, uses corresponding cleaned vocal segment as voice reference
- **Fallback**: If segment < 1.2s, uses longest available segment

### Experimental TS ONNX path

`packages/api/src/ml/voxcpm/` contains a full TypeScript ONNX Runtime implementation:

- 4 ONNX models: prefill, decode_step, vae_encoder, vae_decoder
- EPs: cpu / webgpu
- Not connected to production pipeline

### CosyVoice3 (not yet integrated)

`data/models/CosyVoice3-0.5B/onnx/` has community ONNX exports:

- 14 ONNX models (~3.7 GB), LLM + Flow + HiFT pipeline
- 24kHz output
- RTF ~18-44× CPU (LLM is bottleneck)
- Requires prompt_wav + prompt_text for zero-shot cloning

## Production Models

| Model      | Step         | Runtime  | Device     | Status                |
| ---------- | ------------ | -------- | ---------- | --------------------- |
| Demucs     | 1. Separate  | PyTorch  | CPU        | GPU hang, forced CPU  |
| Whisper    | 2. ASR       | PyTorch  | GPU (cuda) | ✅ Healthy            |
| OpenAI     | 4. Translate | HTTP API | Remote     | ✅ Healthy            |
| VoxCPM     | 6. TTS       | PyTorch  | CPU        | ✅ Active, GPU broken |
| CosyVoice3 | —            | ONNX     | CPU        | Not integrated        |

## GPU Issues (AMD RDNA 3 / ROCm 7.2.3)

- VoxCPM PyTorch GPU: model loads, forward pass → segfault
- ONNX CUDA EP: not available (missing cuDNN 9)
- ONNX WebGPU EP: works for short audio, OOM on medium+ (VK_ERROR_DEVICE_LOST)
- PyTorch Vulkan: not available in current ROCm build
- Whisper CUDA: works fine (only model with working GPU)

See `docs/gpu-status.md` for details.
