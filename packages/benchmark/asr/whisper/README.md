# ASR Whisper Benchmark

## 测试内容
对 `packages/benchmark/video_source.mp4`（170s, AAC 44.1kHz stereo）进行语音识别，对比流水线中实际使用的两个后端：

| 后端 | 路径 | GPU 设备 | CPU 设备 |
|------|------|----------|----------|
| **faster-whisper** | `scripts/asr/run.py` | CUDA float16 | CPU int8 |
| **whisper-pytorch** | `scripts/asr/pytorch.py` | CUDA | CPU |
| **whisper-cli** | `scripts/asr/whisper_cli.py` | ROCm HIPBLAS | CPU (mmap) |

## 使用
```bash
# 跑全部后端（推荐）
bun run packages/benchmark/asr/whisper/run-all.ts

# 仅 faster-whisper
bun run packages/benchmark/asr/whisper/run-fw.ts

# 仅 whisper-pytorch
bun run packages/benchmark/asr/whisper/run-wp.ts

# 仅 whisper-cli (GPU + CPU)
bun run packages/benchmark/asr/whisper/run-whisper-cli.ts
```
结果写入 `results/whisper-bench.json`（`run-all.ts` 合并模式）或各后端独立 JSON 文件。

## 结果 (2026-06-09)

| Engine | Device | Compute | Dur(s) | Load(s) | Proc(s) | RTF |
|--------|--------|---------|--------|---------|---------|-----|
| faster-whisper | cpu | int8 | 170.0 | 5.0 | 98.8 | 0.581 |
| **faster-whisper** | **cpu** | **float32** | **170.0** | **5.0** | **173.9** | **1.023** |
| **whisper-pytorch** | **cpu** | **fp32** | **170.0** | **7.0** | **781.8** | **4.599** |
| **whisper-cli** | **gpu (HIPBLAS)** | **gguf-float16** | **170.0** | **0.5** | **60.7** | **0.357** |
| **whisper-cli** | **cpu (no-gpu)** | **gguf-float16** | **170.0** | **0.7** | **222.8** | **1.310** |
| **whisper.cpp** | **cpu (pywhispercpp)** | **gguf-float16** | **170.0** | **0.9** | **430.0** | **2.529** |

**精度说明：**
- **float32 vs fp32**（同精度）：faster-whisper **4.5× 快**于 whisper-pytorch，无任何偷懒
- **gguf-float16** 是 whisper.cpp 的默认权重格式（fp16），无法用 faster-whisper CPU 复现——CTranslate2 CPU 不支持 float16，仅 GPU 支持
- **int8** 是量化格式，精度有损，列作参考

| 排名 | Engine | RTF | 相对基线 |
|------|--------|-----|---------|
| 1🥇 | whisper-cli (GPU) | **0.357** | 12.9× |
| 2🥈 | faster-whisper (float32) | **1.023** | 4.5× |
| 3🥉 | whisper-cli (CPU) | **1.310** | 3.5× |
| 4 | whisper.cpp (pywhispercpp CPU) | **2.529** | 1.8× |
| 5 | whisper-pytorch (fp32) | **4.599** | 基线 |

### 备注
- **whisper-cli GPU (HIPBLAS)** 是当前最快的 ASR 方案，RTF **0.357**，170s 音频仅需 61s
- **GPU vs CPU 对比**（同一 whisper-cli 二进制）：GPU 比 CPU 快 **3.7×**
- **pywhispercpp CPU 慢于 whisper-cli CPU**（430s vs 223s），推测为 pywhispercpp 的 Python 回调解码开销 + 默认线程数不同
- **whisper-cli 加载最快**（0.5s）：mmap 加载 GGUF 模型
- **模型**：`large-v3-turbo`
- **参考音频**：`packages/benchmark/video_source.mp4`（170s, AAC 44.1kHz stereo）
- **whisper-cli 调用方式**：`submodule/whisper.cpp/build/bin/whisper-cli`，通过 `scripts/asr/whisper_cli.py` 包装（ffmpeg 提取 WAV → whisper-cli）

## 说明
- **模型**：`large-v3-turbo`
- **`--benchmark-load`**：只 `import + load_model`，不 transcribe，独立测加载时间
- **去缓存**：每个测试轮次用唯一 session 目录，跑完删除
- **`run-all.ts` 合并模式**：追加到已有的 `whisper-bench.json`，不会覆盖之前跑过的结果
- 已有文件（`ts-onnx-*.json`、`py-*.json`）不受影响
