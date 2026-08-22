# Demucs fallback

- `shifts=3`, `device="cpu"` — ~1:1 real-time for short audio, scales roughly linearly with duration.
- CrossTransformer + Wiener filter are the primary GPU hang source. Individual CNN layers work fine on GPU with GEMM patch.
