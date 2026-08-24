# Demucs 模型来源

## 代码源

- `submodule/demucs/` — 官方 [adefossez/demucs](https://github.com/adefossez/demucs)，PyTorch 路径使用

## 模型权重

### PyTorch (daemon + subprocess)

| 字段 | 值 |
|---|---|
| 模型名 | `htdemucs_ft` |
| 来源 | HuggingFace hub (`~/.cache/torch/hub/checkpoints/`) |
| 文件 | `htdemucs_ft.th` ~800MB |
| 触发方式 | `demucs.Separator(model="htdemucs_ft")` 时自动下载 |
| 备注 | 无需手动下载，demucs 库自动管理 |

### ONNX (onnxruntime-node)

| 字段 | 值 |
|---|---|
| 模型名 | `htdemucs_ft_vocals.onnx` |
| 来源 | [StemSplitio/htdemucs-ft-onnx](https://huggingface.co/StemSplitio/htdemucs-ft-onnx) (community export, **非官方**) |
| 存储路径 | `DEMUCS_DIR` (由 `@repo/config` 定义) |
| 下载方式 | `downloadDemucs()` — TypeScript 手动调用，非自动 |
