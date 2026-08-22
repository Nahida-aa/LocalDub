# Demucs CPU 优化方案

> 硬件：AMD Radeon 780M (RDNA 3 / gfx1103), 32GB 共享内存
> `HSA_OVERRIDE_GFX_VERSION=11.0.0` 时 rocBLAS 可用，但 MIOpen 无 RDNA 3 预编译数据库
> Demucs 整网跑 GPU 会导致 GPU Hang→黑屏，故固定 CPU

---

## 当前配置

`backend/app/adapters/demucs.py`:

```python
separator = Separator(
    model="htdemucs_ft",
    device="cpu",
    progress=True,
    shifts=3,       # 移位平均 3 次 → 提升 SDR，3× 计算量
)
```

- `segment`：模型默认 7.8s (39/5)
- `overlap`：默认 0.25
- `split`：默认 True
- `jobs`：默认 0（**串行**处理 segment）

当前实测：37min 视频分离耗时约 110min（~3× 实时）。

---

## 方案一：`jobs=N` 并行（零质量影响，一行代码）

### 原理

`Separator` 接受 `jobs` 参数，传递给 `apply_model` 的 `num_workers`：

```python
# apply.py
if pool is None:
    if num_workers > 0 and device.type == 'cpu':
        pool = ThreadPoolExecutor(num_workers)   # 真正的多线程
    else:
        pool = DummyPoolExecutor()               # 串行
```

- `jobs=0`：所有 segment 串行推理（当前行为）
- `jobs=4`：最多 4 个 segment 同时推理

### 为什么可行

- PyTorch 的 `model.forward()` 在 C++ 层释放 GIL，多线程可真正并行
- 模型参数只读共享，不额外占用内存
- `jobs` 只影响并行度，不影响任何数值结果

### 实测（htdemucs, 5min 音频, shifts=3, AMD Radeon 780M）

| jobs | Wall time | RTF | vs jobs=0 |
|------|-----------|-----|-----------|
| 0 | 585s | 1.951 | baseline |
| 4 | 538s | 1.795 | ~8%  |

**结论**：`jobs=N` 在 CPU 上收益有限（~8%），因为：
- PyTorch 模型 forward 并非在所有算子上都释放 GIL
- 多线程竞争 CPU 缓存和内存带宽（DDR5 共享）
- DummyPoolExecutor 退化为串行（当某些操作不释放 GIL 时）

对于 37min 视频，预期从 ~110min 降至 ~100min，不值得为 ~10% 加速增加代码复杂度。

### 改动

```python
separator = Separator(
    model="htdemucs_ft",
    device="cpu",
    progress=True,
    shifts=3,
    jobs=4,             # 新增
)
```

---

## 方案二：减小 `segment` 降低注意力计算量

### 原理

CrossTransformer 的自注意力和交叉注意力复杂度为 O(T²)，其中 T 为每个 segment 的时间帧数。

当前 `segment=7.8s`：
- Freq 分支：8 freq × 336 time = 2688 tokens
- Time 分支：1344 tokens
- 每层注意力：QK^T 约 4032² ≈ 16M 点积

减小 `segment` 后：
- T 线性减少 → 注意力 O(T²) 二次方减少
- segment 数量增加，但注意力总计算量仍大幅下降

### 预估效果

| segment | tokens (freq+time) | 注意力 O(T²) | segment 数 | 总计算量 |
|---------|-------------------|-------------|-----------|---------|
| 7.8s | ~4032 | 16.3M | ~388 | 100% |
| 5.0s | ~2688 | 7.2M | ~605 | 107% |
| 3.0s | ~1680 | 2.8M | ~1008 | 115% |

> 注：segment 缩小后 segment 数增加，最终总计算量可能不降反升。
> 实际效果需测试——过小的 segment 会增加 overlap 处理开销。

### 改动

```python
separator = Separator(
    model="htdemucs_ft",
    device="cpu",
    progress=True,
    shifts=3,
    segment=5.0,        # 需要尝试不同值
)
```

---

## 方案三：GGML 格式 + Candle (Rust) 推理

### 原理

用 Candle（Rust ML 框架）加载 Demucs，完全绕过 PyTorch 和 ROCm：

- Conv1d/Conv2d → GEMM（candle 内置 matmul）
- CrossTransformer → candle 的 `Module` + `attn`
- 无 Python GIL 开销
- 无 ROCm/MIOpen 兼容问题
- 可通过 `rayon` 自动利用所有 CPU 核

### 数据流

```
GGML 量化模型 ←── PyTorch 权重导出
         ↓
   Candle 推理（纯 Rust）
         ↓
   输出 WAV Tensor
```

### 重点：复用 `conv_patch.py` 的接口

当前 `backend/app/adapters/conv_patch.py` 提供了完整的 GEMM 卷积实现：

```
F.conv1d            → _conv1d_gemm           → Rust 实现
F.conv2d            → _conv2d_gemm           → Rust 实现
F.conv_transpose1d  → _conv_transpose1d_gemm → Rust 实现
F.conv_transpose2d  → _conv_transpose2d_gemm → Rust 实现
```

通过 `apply_patch()` 即可全局替换。这一步已经在 Python 侧验证了正确性。

Rust 侧可分别用 PyO3 FFI 暴露：

```rust
#[pyfunction]
fn conv1d_gemm(input: PyObject, weight: PyObject, ...) -> PyResult<PyObject> { ... }
```

或者直接用 Candle 的 `conv1d` / `matmul` 原生实现。

### CrossTransformer 的 Rust 实现

GPU Hang 的根源在 CrossTransformer 的 attention。Rust 实现需要：

1. LayerNorm → candle `layer_norm`
2. Self-attn (causal) → candle `attn` or 手写 `bmm(softmax(q @ k.T / sqrt(d)), v)`
3. Cross-attn → q_freq @ k_time.T 后 bmm
4. FFN (Linear + GELU) → candle `linear` + `gelu`
5. Dropout → 推理时可省略

### 实现路径

1. 先用 Python 导出权重为 safetensors 或 GGML 格式
2. 用 Candle 加载，实现 forward 一样的前向
3. 用 PyO3 封装为 Python 可调用的 Rust 模块
4. 替换 `demucs.py` 中的 `Separator` 调用

---

## 方案四：ONNX Runtime 推理

- 将 Demucs 导出为 ONNX
- 用 ONNX Runtime 的 CPU execution provider 推理
- 自动利用所有 CPU 核 + 指令集优化
- 实现更简单（不需要 Rust），但优化空间受限于 ONNX Runtime

---

## 方案对比

| 方案 | 加速比 | 质量影响 | 实现成本 | 备注 |
|------|--------|---------|---------|------|
| `jobs=N` | 2-4× | 无 | 1 行 | 立刻可用 |
| 调 `segment` | 不确定 | 无 | 1 行 | 需实测 |
| Candle Rust | 5-10× | 无 | 高 | 长远最优 |
| ONNX Runtime | 3-5× | 无 | 中 | 折中方案 |

---

## 相关文件

- `backend/app/adapters/demucs.py` — Demucs 适配器，优化入口
- `backend/app/adapters/conv_patch.py` — GEMM 卷积参考实现，Rust 迁移的接口蓝本
- `submodule/demucs/demucs/apply.py` — `apply_model` 入口，`num_workers` 和 `ThreadPoolExecutor` 在此
- `submodule/demucs/demucs/htdemucs.py` — 模型定义，`segment`、`cac`、`wiener_iters` 等参数
- `submodule/demucs/demucs/api.py` — `Separator` 类，封装了 `apply_model` 调用
- `submodule/demucs/demucs/transformer.py` — CrossTransformer 实现，GPU Hang 根因
- `AGENTS.md` — 硬件信息和已知问题记录
