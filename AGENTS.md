# LocalDub

测试/实验/探索一律不用 `/tmp`，写到 `packages/tmp/`。

## Key directories

- `packages/cli/src/feat/` — pipeline 流程（stages、config、tasks）
- `packages/cli/src/ml/` — 模型实现（whisper、demucs 等）
- `packages/benchmark/` — 性能测试与参数对比
- `submodule/whisper.cpp/` — whisper.cpp 官方仓库（GPU Vulkan 构建 → `build/bin/whisper-vulkan`）
- `.agents/hardware.md` — 硬件兼容性 & 已知失败路径
- `.agents/model-strategy.md` — 各模型设备分配详情

## Model assignment (active paths only)

| 模型 | 设备 | 说明 |
|------|------|------|
| whisper.cpp ASR | Vulkan (RADV) | **实际路径**，RTF ~0.09-0.11 (参数相关)，large-v3-turbo |
| Demucs (ONNX, onnxruntime-node) | CPU | **实际路径**，RTF ~1.0 |
| CosyVoice3 TTS | CPU | 无可用 GPU EP |
| 翻译 | CUDA | 正常 |

❌ 失败的：whisper-pytorch GPU segfault、faster-whisper GPU 缺 libcuda.so、whisper.cpp HIPBLAS MES hang、VoxCPM 所有路径均已废弃。详细原因 → `.agents/hardware.md` / `.agents/model-strategy.md`

## Temp directory

- `packages/tmp/` — 临时文件/构建产物（gitignored via `*/tmp/*`）
- 大型第三方编译（如 ORT 源码）放此目录而非系统 `/tmp/`

## Known limits

- **OOM**: daemon RSS > 9.5GB 时可能 OOM。详情 → `packages/research/model-load-benchmarks.md`
- **Dawn WebGPU**：≥3 sessions → `VK_ERROR_DEVICE_LOST`，限制 ≤2 个 WebGPU session
- **ffmpeg swresample whisper 幻觉循环**：sidechain 混音音频在 ffmpeg `-ar 16000` 后尾段产生 x68+ 幻觉循环。已去掉该冗余重采样，让 miniaudio 内部处理。详情 → `.agents/asr-loop-fix.md`
- **whisper.cpp 无法检测短语音**：0.5s+ 的短叹（"唉" 71.20）和轻笑（"哈哈哈" 115.42）在 38 个参数组合中几乎全部 miss。silero VAD v6 能捕获"唉"但 CER 涨 3-4ppt 且时间戳左漂 0.8-1.2s；"啊+哈哈哈"则没有任何参数能捕获——whisper 语言模型解码偏好将短语音合并入相邻段。详情 → `packages/benchmark/asr/whisper/results/FINDINGS.md`
- **VAD 变体时间戳偏移**：所有 VAD 模式都系统性地将分段边界左移（s_off_mean -0.75~-1.85s），导致字幕 timing 不准。CER 最低的 sidechain+vad-v6-th02（7.00%）偏移 -0.76s。最佳平衡参数是 sidechain+temp-02（CER 7.72%，s_off +0.10s，557/557 字符）

## OCR Pipeline

| 引擎 | 备注 | CER (fps1 sub-only) | RTF |
|------|------|:-------------------:|:---:|
| Python (rapidocr) | 参考基线, perspective-warped crop | 2.11% | 0.538 |
| Node.js (onnxruntime-node) | 2.0× faster, AABB crop | 4.92% | 0.263 |
| **C++ ORT (推荐)** | **2.9× faster, 无 Python 依赖, CER 超越 Python(1.93% vs 2.11%)** | **1.93%** | **0.186** |
| C++ ORT (0.5fps) | RTF 接近 ASR | 21.79% | 0.092 |

C++ ORT 为生产推荐路径。详情 → `packages/benchmark/ocr/results/FINDINGS.md`

## TODO

- 自动设备检测：目前 default `device: "cuda"` 在 ROCm 上可能 hang，待实现运行时可感知的自动检测
- ASR 参数基准测试完成：sidechain + vad-v6-th02 最佳 CER 7.00% 但 s_off -0.76s；sidechain + temp-02 最佳字符数 557/557 且 s_off +0.10s。详情 → `packages/benchmark/asr/whisper/results/FINDINGS.md`
- OCR 集成到 pipeline `ocr.ts` stage（当前为占位符）

## Navigation

- `.agents/hardware.md` — GPU 兼容性 & MES hang 根因
- `.agents/model-strategy.md` — 各模型设备分配策略 & 废弃路径详情
- `.agents/demucs.md` — Demucs CPU fallback 说明
- `.agents/cosyvoice2.md` — CosyVoice2/3 ONNX 导出状态
- `.agents/asr-loop-fix.md` — ffmpeg swresample 导致 whisper 幻觉循环根因
- `docs/webgpu-oom.md` — WebGPU `VK_ERROR_DEVICE_LOST` 根因分析
- `packages/benchmark/asr/whisper/results/FINDINGS.md` — ASR 参数基准测试详细结果（38 组合）
