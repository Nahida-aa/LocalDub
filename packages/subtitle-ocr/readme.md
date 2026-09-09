## OCR Pipeline

| 引擎 | 配置 | normCER | 段数 | s_off | cover | FP | RTF |
|------|------|:-------:|:----:|:-----:|:-----:|:--:|:---:|
| C++ ORT (OpenCV) | 2fps, so, ts0.45 | **0%** | 75 | +228ms | 68.2% | 0 | 0.186 |
| Python (rapidocr) | 1fps, so | 0.89% | 74 | -39ms | — | 0 | 0.538 |
| Python (rapidocr) | 1fps | 1.25% | 72 | -159ms | — | 0 | 0.538 |
| C++ ORT (OpenCV) | 1fps, so, ts0.3 | 0.89~3.76% | 74-75 | -26ms | — | 0 | 0.186 |
| C++ ORT (OpenCV) | 1fps, so, ts0.4 | 0.89~3.40% | 74-75 | -26ms | — | 0 | 0.186 |
| C++ ORT (OpenCV) | 1fps, so, ts0.45 | 0.89% | 74 | -39ms | — | 0 | 0.186 |
| C++ ORT (OpenCV) | 1fps, so | 1.07~5.37% | 74-76 | -26ms | — | 0 | 0.186 |
| Node.js (onnxruntime-node) | 1fps, so | 3.58% | 74 | -119ms | — | 0 | 0.263 |
| C++ ORT (OpenCV) | 0.5fps, so | 20.75% | 54 | -639ms | — | 0 | 0.092 |

C++ ORT (OpenCV) 方差全来自 ORT 多线程 run-to-run 非确定性（~0.89-5.37%），**ts 参数在 subtitleOnly 下影响被波动淹没**（27 次运行 FP=0）。所有引擎的 `subtitleOnly` → `textScore=0.3` override 已移除（C++ `ocr_pipeline.cpp:331`、Python `subtitle-py.py:29-30`、Node `subtitle-node.ts:171`），`subtitleOnly` 现在只做 Y 轴裁剪。

**Cover 指标**：仅是描述性数据，标识检测到的段覆盖了多少时间线比例，**并非越高越好**。自然的口播字幕之间有空白间隙，100% cover 意味着段完全连续无间隙，只是反映结果的时间特性。

详情 → `vox-lab/packages/benchmark/ocr/results/FINDINGS.md`，结果已统一用 `eval-ocr.ts --ms` 评估（OCR 时间戳为毫秒）。
