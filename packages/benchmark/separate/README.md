# Separate Benchmark — Demucs 音频分离

## 测试内容
对三段不同时长（10s / 60s / 120s）的音频（粉红噪声，16-bit 44.1kHz 单声道 WAV）进行 Demucs 4-stem 分离，提取人声 + 背景音。对比三个后端，支持 `shifts=1`（公平对比）和 `shifts=3`（高质量）：

| 后端 | 路径 | 模型加载方式 | 推理引擎 | 支持 shifts |
|------|------|------------|---------|------------|
| **PyTorch** | `scripts/separate/run.py` | 每次子进程从头加载 | PyTorch CPU | 1 / 3（通过 `--shifts`） |
| **ORT** | `Demucs` class (onnxruntime-node) | InferenceSession 创建 | onnxruntime-node CPU | 无 shift 机制 |
| **GGML** | `demucs_mt.cpp.main` (C++) | 每次子进程加载 | Eigen + OpenBLAS + OpenMP | 仅 shift=1（硬编码） |

**RTF 定义**：三个后端统一为 `processTimeS / durationS`，其中 `processTimeS = totalTimeS - loadTimeS`（不含模型加载时间）。

## 使用
```bash
# 跑全部后端
bun run packages/benchmark/separate/run-all.ts

# 仅 GGML（推荐先装 openblas）
OMP_NUM_THREADS=2 bun run packages/benchmark/separate/run-ggml.ts

# 仅 ONNX
bun run packages/benchmark/separate/run-onnx.ts

# 仅 PyTorch（默认 shifts=3）
bun run packages/benchmark/separate/run-pytorch.ts

# 视频实测：对比 GGML (shift=1) vs PyTorch (shifts=1/3)，保存 WAV
bun run packages/benchmark/separate/run-video.ts

# ORT 视频实测
bun run packages/benchmark/separate/run-video-onnx.ts

# WER/CER 质量对比（需 whisper-cli + ground truth）
bun run packages/benchmark/separate/run-wer.ts
```

结果写入 `results/separate-bench.json`。

## 当前结果 (2026-06-09)

### 视频实测对比（`video_source.mp4` 170.1s 立体声）

以人工标注转录为 ground truth，whisper.cpp (large-v3-turbo) 对分离后 vocals 转写的准确率。整合速度与质量指标，含 LLM 纠错后结果（`gemma4:31b`）：

| 后端 | 设备 | Shifts | RTF | CER 裸 | CER+LLM | 推荐 |
|------|------|--------|-----|--------|---------|------|
| **原始视频** | — | — | **0** | 7.54% | 4.13% | baseline |
| **GGML** | cpu | 1 | 1.362 | 8.44% | 4.49% | |
| **ORT** | cpu | 无 | **1.340** | 8.08% | 4.13% | ⚡ **最快** |
| **PyTorch** | cpu | 1 | 2.647 | 7.18% | 3.77% | |
| **PyTorch** | cpu | 3 | 7.922 | 6.64% | **3.05%** | CER 最低 |

> **推荐：不分离 → ORT → GGML** — LLM 纠错后各版本 CER 差距缩小到 1.44pp 以内，速度成为主要决策因素。原始音频 CER 4.13% 已非常好；如果有 dubbing 需求需提取人声，ORT 速度最快、CER 最低（与 raw 并列 4.13%）。

- **LLM 纠错效果显著**：CER 降低 45-55%，且**抹平了各后端之间的质量差距**（LLM 前 max-min = 1.8pp，LLM 后 = 1.44pp）
- **GGML WAV 格式**：默认输出 32-bit float，whisper.cpp 对其转录质量会显著下降（CER 14.72%），需转 16-bit

### GGML 合成音频（OpenBLAS + MT，Ryzen 7 H 255 8c/16t）

| Engine | Device | Audio | Dur(s) | Load(s) | Proc(s) | RTF |
|--------|--------|-------|--------|---------|---------|-----|
| ggml | cpu | short | 10.0 | 0.2 | 28.8 | 2.878 |
| ggml | cpu | medium | 60.0 | 0.2 | 83.7 | 1.395 |
| ggml | cpu | long | 120.0 | 0.2 | 169.5 | 1.412 |
| ggml | cpu | video_source.mp4 | 170.1 | 0.2 | 235.6 | 1.386 |

**SMT 对比**（video_source.mp4 170.1s）：

| 配置 | 总线程 | Wall(s) | Proc(s) | RTF | Speedup |
|------|--------|---------|---------|-----|---------|
| mt=4, OMP=2 | 8 (物理核) | 235.4 | 235.4 | 1.384 | 1.000 |
| mt=4, OMP=4 | 16 (含 SMT) | 224.4 | 224.4 | 1.319 | 1.049 |

SMT 超线程对 Demucs 密集矩阵运算帮助有限（约 5%），这是因为 Demucs 的 Eigen 矩阵乘是**内存带宽瓶颈**，额外逻辑核共享 L2 cache 和 FPU，收益很小。`OMP_NUM_THREADS=2`（仅物理核）是性价比最优策略。

### ORT + PyTorch 合成音频（旧数据，归一化修正前）

**注意**：PyTorch 旧数据为 `shifts=3`（默认值），新数据需用 `--shifts` 重跑。ORT 视频 RTF 见上表。

| Engine | Device | Audio | Dur(s) | Load(s) | Proc(s) | RTF |
|--------|--------|-------|--------|---------|---------|-----|
| ort | cpu | short | 10.0 | 16.9 | 21.6 | 2.161 |
| ort | cpu | medium | 60.0 | 16.9 | 142.7 | 2.379 |
| ort | cpu | long | 120.0 | 16.9 | 258.8 | 2.157 |
| pytorch | cpu | short | 10.0 | 2.3 | 107.1 | 10.712 |
| pytorch | cpu | long | 120.0 | 2.3 | 1422.3 | 11.853 |

### 备注
- **PyTorch medium 旧数据失败**（91% 时出错），short & long 有效
- **加载时间**：GGML 0.2s vs ONNX 16.9s vs PyTorch 2.3s（GGML 是 C++ 裸加载最快；PyTorch 含 Python import 开销；ONNX 含 ONNX 文件验证 + session 创建）
- **GGML 外归一化**：`model_apply.cpp` 实现了和 `api.py` 一致的 mono mix → scalar mean/std，输出音量与 PyTorch 一致
- **GGML 线程策略**：mt=4 std::thread + OMP_NUM_THREADS=2 = 8 线程（物理核数），避免 SMT 竞争 L2 cache
- **Shifts 参数**：`--shifts N` 仅在 PyTorch 后端支持（`_engine.py` / `run.py`），GGML 硬编码 shift=1，ORT 无 shift 机制。`shifts=3` 是 PyTorch 默认值，`shifts=1` 用于公平对比
- **合成 vs 视频**：GGML 在视频（170s 立体声）上 RTF 1.362，与合成数据一致

## 环境
- APU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (8c/16t, Zen4)
- ROCm 7.2.3
- onnxruntime-node 1.26.0
- openblas 0.3.33
- Demucs submodule (htdemucs_ft)
- demucs.cpp submodule (ggml + Eigen + OpenBLAS + OpenMP)
