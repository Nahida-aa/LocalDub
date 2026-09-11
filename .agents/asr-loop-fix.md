# ASR Loop Fix — ffmpeg swresample 导致 whisper 幻觉循环

## 问题

Sidechain 混音后的 `target_3_vocals_mixed.wav`（44.1kHz stereo），pipeline 的 `asrWhisperCpp` 先用 ffmpeg 转 16kHz mono，再用 whisper.cpp Vulkan 转录，结果尾段（~150-170s）产生幻觉循环（单句重复 68+ 次）。

## 证据链

| 输入 | 重采样方式 | 结果 |
|------|-----------|------|
| 44.1kHz stereo | miniaudio 内部 | ✅ 89 segs，内容正确 |
| 44.1kHz mono (`-ac 1` 无重采样) | miniaudio 内部 | ✅ 93 segs，无循环 |
| 16kHz mono (`-ar 16000 -ac 1`) | **ffmpeg swresample** | ❌ 123 segs，x68 循环 |
| 16kHz stereo (`-ar 16000` 只重采样) | **ffmpeg swresample** | ❌ 126 segs，x71 循环 |
| 16kHz mono via soxr resampler | ffmpeg soxr | ❌ 104 segs，x23 循环（稍好仍不行） |

所有 16kHz 版本 RMS 水平与 44.1kHz 版本一致（~-25.5dB），不是静音问题。

## 根因

1. **whisper.cpp README 官方推荐** `ffmpeg -i input.mp3 -ar 16000 -ac 1 -c:a pcm_s16le output.wav` 作为预处理。`asr.ts:346` 当初加 `-ar 16000` 是遵循此标准，不是为了降 CER（无任何 markdown 记载重采样能降 CER）。

2. `common-whisper.cpp:96` 中 `read_audio_data` 已强制 miniaudio 输出 `WHISPER_SAMPLE_RATE=16000`：
   ```cpp
   decoder_config = ma_decoder_config_init(ma_format_f32, stereo ? 2 : 1, WHISPER_SAMPLE_RATE);
   ```
   所以 **ffmpeg 的 `-ar 16000` 是完全冗余的**——传 44.1kHz 给 whisper，miniaudio 内部也会重采样到 16kHz。

3. **ffmpeg swresample (sinc) 比 miniaudio 插值重采样保留更多高频 content**。Sidechain 混音后尾段的 artifact 频段本应被低通滤波消除，swresample 保留了它，whisper 将其误判为重复语音 → 幻觉循环。miniaudio 的简单插值重采样起到无意低通滤波作用，移除了触发频段。

## 修复

`asr.ts:346`：

```diff
-spawnSync('ffmpeg', ['-y', '-i', audioPath, '-ar', '16000', '-ac', '1', tmpAudio], {
+spawnSync('ffmpeg', ['-y', '-i', audioPath, '-ac', '1', tmpAudio], {
```

只保留 `-ac 1`（强制 mono），去掉 `-ar 16000`，让 miniaudio 处理重采样。whisper 永远工作在 16kHz（由 miniaudio 保证），不受影响。

## 相关文件

- `packages/cli/src/feat/steps/asr.ts:346` — 修复位置
- `vox-lab/submodule/whisper.cpp/examples/common-whisper.cpp:96` — miniaudio 强制 16kHz 输出
- `vox-lab/submodule/whisper.cpp/README.md:102` — 官方预处理示例
