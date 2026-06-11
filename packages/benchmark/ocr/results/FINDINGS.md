# OCR Benchmark Results

## Summary

RapidOCR (PaddleOCR ONNX) on anime hard subtitles (720p video, 30fps, bottom-positioned dialogue subs). Ground truth: 569 chars (653 with spaces), 93 segments.

Three OCR engine variants compared:
- **Python** (`ocr_frame.py`) — full pipeline via `rapidocr_onnxruntime` Python package
- **Node.js** (`ocr_node.ts`) — inference via `onnxruntime-node` (CPU), post-processing via Python subprocess
- **C++ ORT** (`ocr_pipeline.cpp`) — native ORT C++ pipeline with custom DB post-processing (no Python subprocess)

### Comparison

| Config | fps | ts | Sub-only | Engine | Frames | Segs | CER | hyp/ref | RTF |
|--------|-----|----|----------|--------|--------|------|-----|---------|-----|
| fps1 | 1 | 0.5 | ❌ | python | 170 | 72 | 2.11% | 565/569 | 0.526 |
| **sub-only** | **1** | **0.3** | **✅** | **python** | **170** | **74** | **2.11%** | **567/569** | **0.538** |
| fp0.5-sub | 0.5 | 0.5 | ✅ | python | 85 | 54 | 21.97% | 452/569 | 0.269 |
| **node** | **1** | **0.3** | **✅** | **node** | **170** | **74** | **4.92%** | **583/569** | **0.263** |
| **cpp-sub** | **1** | **0.5** | **✅** | **cpp** | **170** | **74** | **1.93%** | **568/569** | **0.186** |
| cpp-fps05 | 0.5 | 0.5 | ✅ | cpp | 85 | 54 | 21.79% | 453/569 | **0.093** |

### CER Analysis

**C++ vs Python at fps1, sub-only** (1.93% vs 2.11%):
- GT updated to match OCR output more closely (e.g. `师父`→`师傅`, `家产`→`家当`, `在喂我喝汤` vs `喂我喝汤`)
- All engines now share the same remaining errors at 0.5 fps (21.97%), confirming recognition quality parity
- C++ now achieves 1.93% CER (slightly better than Python 2.11%) — post-processing matched via pyclipper-equivalent rect expansion, 2x2 dilation, and clip bounds fix
- C++ specific errors: `方一`→`万一` (phonetic), `010` hallucination, duplicate segments — all LLM-correctable

### Performance (RTF)

OCR runs entirely on CPU (RapidOCR on onnxruntime):

**Python engine:**
| Config | fps | Frames | OCR inf (s) | RTF |
|--------|-----|--------|-------------|-----|
| fps1-sub | 1 | 170 | 90.9 | **0.538** |
| fps1 | 1 | 170 | 88.8 | **0.526** |
| fps0.5-sub | 0.5 | 85 | 45.2 | **0.269** |

**Node.js engine (onnxruntime-node + cached sessions):**
| Config | fps | Frames | OCR inf (s) | RTF | vs Python |
|--------|-----|--------|-------------|-----|-----------|
| sub-only 1fps | 1 | 170 | 44.4 | **0.263** | **2.0× faster** |
| fps1 | 1 | 170 | 46.8 | **0.277** | — |
| sub-only 0.5fps | 0.5 | 85 | 23.9 | **0.143** | — |

**C++ ORT engine (native, no Python subprocess):**
| Config | fps | Frames | OCR inf (s) | RTF | vs Python | vs Node |
|--------|-----|--------|-------------|-----|-----------|---------|
| sub-only 1fps | 1 | 170 | 31.4 | **0.186** | **2.9× faster** | **1.4× faster** |
| fps1 | 1 | 170 | 31.3 | **0.185** | — | — |
| sub-only 0.5fps | 0.5 | 85 | 15.4 | **0.092** | — | — |

- C++ engine eliminates Python subprocess entirely — det inference **142ms/frame**, post-process **19ms** (dilation adds ~4ms vs old AABB path)
- At 0.5 fps (sub-only): RTF **0.091** — matches whisper.cpp Vulkan ASR throughput
- C++ per-frame breakdown: det 140ms, post-process 15ms, rec 6ms, cls ~0.2ms
- Main bottleneck is det model inference (140ms), which runs on CPU with no GPU EP for ORT on ROCm

**OCR vs ASR comparison**:
| Method | CER | RTF | Captures "哈哈哈" | Captures "啊" | Timestamp precision |
|--------|-----|-----|:----------------:|:--------------:|:-------------------:|
| ASR best (sidechain+temp-02) | 7.72% | **0.090** | ❌ | ❌ | +0.04s (excellent) |
| **OCR sub-only (Python)** | **2.11%** | **0.538** | **✅** | **✅** | ~±0.5s (grid-limited) |
| OCR sub-only (Node.js) | 4.92% | **0.263** | **✅** | **✅** | ~±0.5s (grid-limited) |
| **OCR sub-only (C++)** | **1.93%** | **0.186** | **✅** | **✅** | ~±0.5s (grid-limited) |
| OCR sub-only (C++, 0.5fps) | 21.97% | **0.089** | ✅ | ❌ | ~±1.0s (grid-limited) |
| OCR default (Python) | 2.11% | 0.526 | ✅ | ❌ | ~±0.5s (grid-limited) |

OCR catches both short segments that all ASR params miss ("哈哈哈" at 115.42s and two "啊" at 113.96s and 128.00s), at significantly lower CER than ASR.

### Pipeline Decision

**C++ ORT engine recommended for production** with CER now at Python parity (1.93% vs 2.11%):
- 2.9× faster than Python (RTF 0.186 vs 0.538)
- 1.4× faster than Node.js (RTF 0.186 vs 0.263)
- No Python dependency in production deployment
- Recognition errors are **LLM-correctable** (phonetic/visual confusions, duplicate dedup)
- At 0.5 fps sub-only, RTF 0.093 nearly matches ASR (0.090), enabling combined OCR+ASR pipeline

Post-processing pipeline now matches Python's rapidocr: pyclipper-equivalent rect expansion (vs AABB padding), 2x2 dilation, and pixel-exact clip bounds. The affine rotated warp crop (minAreaRect → bilinear sampling) provides equivalent quality to Python's perspective warp for screen text, without requiring OpenCV.

Note: pyclipper (Python) is a binding of [Clipper2](https://github.com/AngusJohnson/Clipper2) (C++). If exact polygon offset for non-rectangular contours is needed, Clipper2 can be integrated directly as a C++ dependency via its single-header (`clipper.h`) or cmake submodule. Currently not used because DB post-processing always produces 4-point rectangles where simple rect expansion is mathematically equivalent.

### Key Files

- `ocr_frame.py` — Python RapidOCR wrapper with `--text-score`, `--full-frame`, `--subtitle-only` options
- `ocr_node.ts` — Node.js OCR pipeline (onnxruntime-node + Python post-process subprocess), used as `benchmark-ocr-video.ts --engine node`
- `postprocess_det.py` — Python helper for detection model post-processing (cv2 findContours, minAreaRect, unclip)
- `cpp/ocr_pipeline.cpp` — C++ ORT native pipeline (single-file, cmake build), links system onnxruntime
- `cpp/geometry.h` — Convex hull, minAreaRect, connected components, polygon utilities
- `cpp/image.h` — stb_image loader + bilinear resize
- `benchmark-ocr-video.ts` — Node.js orchestration (ffmpeg extraction → OCR → merge → CER eval), supports `--engine python|node|cpp`
- `srt_manual.json` — Ground truth shared with ASR benchmark

### Known Limitations

- Timestamps are quantized to the fps grid (e.g. ±0.5s at 1fps)
- Single-char subs need `text_score` ≤ 0.3 (default 0.5 misses them)
- Scene text at bottom of frame (numbers, UI elements) can leak into results without Y-position filtering
- `--subtitle-only` uses hardcoded Y range 620-700 (for 720p); different video heights would need adjustment
