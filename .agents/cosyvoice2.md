# CosyVoice ONNX Status

## CosyVoice3 ONNX（当前主力）

社区 `ayousanz/cosy-voice3-onnx` 已有完整 ONNX 全链路（14 个文件，~3.7 GB）。

### 模型文件

`data/models/CosyVoice3-0.5B/onnx/`

| 阶段               | 文件                                | 大小   | 格式 |
| ------------------ | ----------------------------------- | ------ | ---- |
| 声纹               | `campplus.onnx`                     | 27 MB  | FP32 |
| 语音 token         | `speech_tokenizer_v3.onnx`          | 925 MB | FP32 |
| 文本编码           | `text_embedding_fp32.onnx`          | 519 MB | FP32 |
| LLM prefill        | `llm_backbone_initial_fp16.onnx`    | 684 MB | FP16 |
| LLM decode         | `llm_backbone_decode_fp16.onnx`     | 684 MB | FP16 |
| LLM decoder        | `llm_decoder_fp16.onnx`             | 12 MB  | FP16 |
| LLM speech emb     | `llm_speech_embedding_fp16.onnx`    | 12 MB  | FP16 |
| Flow token emb     | `flow_token_embedding_fp16.onnx`    | 1 MB   | FP16 |
| Flow estimator     | `flow.decoder.estimator.fp16.onnx`  | 634 MB | FP16 |
| Flow pre-lookahead | `flow_pre_lookahead_fp16.onnx`      | 1 MB   | FP16 |
| Flow speaker proj  | `flow_speaker_projection_fp16.onnx` | 0 MB   | FP16 |
| HiFT F0            | `hift_f0_predictor_fp32.onnx`       | 13 MB  | FP32 |
| HiFT source        | `hift_source_generator_fp32.onnx`   | 247 MB | FP32 |
| HiFT decoder       | `hift_decoder_fp32.onnx`            | 67 MB  | FP32 |

### 推理脚本

`data/models/CosyVoice3-0.5B/onnx/scripts/onnx_inference_pure.py`（889 行，零 PyTorch 依赖）

条件：

- `pip install onnxruntime==1.18.0 numpy==1.26.4`
- `prompt_wav` + `prompt_text` 必填（zero-shot voice cloning）

### CPU 基准测试（RDNA 3 / AMD 780M）

| 文本              | 生成 (s) | 音频 (s) | RTF  | 瓶颈                                  |
| ----------------- | -------- | -------- | ---- | ------------------------------------- |
| short (19 chars)  | 51.5     | 1.16     | 44.3 | LLM 12.8s / Flow 36.9s / HiFT 0.6s    |
| medium (90 chars) | 113.2    | 5.08     | 22.3 | LLM 50.4s / Flow 59.9s / HiFT 2.5s    |
| long (288 chars)  | 378.2    | 20.0     | 18.9 | LLM 212.9s / Flow 155.0s / HiFT 10.0s |

- HiFT 上采样率 `[8,5,3]` = 120×（CV2 是 `[16,11,7]` = 1232×）
- RTF 随文本变长改善（批处理效率提高）
- 无 CUDA（缺少 cuDNN 9 + CUDA 12），全部 CPU EP

### 已知问题

- 需 onnxruntime==1.18.0（1.26.0 下加载 FP16 LLM 模型报错）
- 模型路径构造：`onnx_dir = join(model_dir, 'onnx')`，默认 `model_dir = pretrained_models/Fun-CosyVoice3-0.5B`
- 采样率：24000 Hz

## CosyVoice2 ONNX（已完成，归档）

`pretrained_models/CosyVoice2-0.5B/`

- `hift_lourdle.onnx` (symlink `hift.onnx`) — 80 MB，已验证 T=50~200 推理正确
- `flow_fp32.onnx`, `flow_fp16.onnx` — 加载通过但从未端到端接入
- `flow_hift_combined_fp32.onnx`, `flow_hift_combined_fp16.onnx` — 加载通过

**结论**：CosyVoice2 缺少社区 ONNX 全链路（无 LLM ONNX 导出）。已切换至 CosyVoice3（社区提供完整 ONNX 全链路）。

### 参考导出脚本

`vox-lab/packages/benchmark/VC/CosyVoice2/lourdle_ref/`
