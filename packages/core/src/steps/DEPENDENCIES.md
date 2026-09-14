# Step 读写契约（依赖关系）

> **这份文档回答一个问题：改某个 step 时，我怎么知道会不会弄坏别处？**
>
> 依赖**不是声明出来的**，而是体现在**文件系统**上：一个 step 读的文件，是另一个
> step 写的。所以本文只记两件事——**每个 step 读什么、写什么**。
>
> 有了这两列，「谁依赖谁」是可以推导的：**A 依赖 B ⟺ A 读的某个路径落在 B 的产出里**。

## step 名单的唯一真源

`StepName`（`workflows/args.rs:46`）**是编译期真源**，另外两处都从它派生：

| 位置 | 角色 |
| --- | --- |
| `args.rs` `enum StepName` | **唯一真源**（15 个变体，serde/clap/specta 共用） |
| `args.rs` `as_str()` / `ALL_STEPS` | 从枚举派生（`match` 穷尽） |
| `pipeline.rs` `run_step` | 从枚举派生（`match` 穷尽，**无 `_` 兜底**） |
| `steps.rs` `*_STEPS` 常量 | 字符串（便于与 TS 逐字对照），由 `get_steps` **解析成枚举** |
| `steps.rs` `STEPS_LIST` | 字符串（对外校验用），有测试钉住它与 `ALL_STEPS` 一致 |

**意义**：给枚举加一个变体却忘了登记 handler，**编译会失败**——不会再出现
「改了一处、另一处静默跳过」。三处曾有各自独立的名单，靠人工保持一致。

## 为什么需要这份文档

依赖藏在代码细节里（路径 helper 多层间接、配置可覆盖、条件分支），
`grep` 一个名字**搜不到数据流**。曾因此得出错误结论（以为 `separate_after` 在
sf_ocr 流里无人消费，实际 `mix_video` 通过 `bgm_path()` 读它的产物）。

所以本文的记法是**列路径**，而不是画箭头——路径是事实，箭头是推断。

## 怎么读

- **读 / 写** 两列是**路径**（相对 `video_dir`），不是「step 名」。
- **读**列标 ⚠️ 的表示**缺失即失败**；其余是「有则用，无则回退」。
- 条件分支用 `[…]` 标出，因为**依赖随参数变化**——同一个 step 在不同配置下
  读的东西不同。这正是静态图不可靠的原因。

---

## 输入（不属于任何 step 产出）

| 路径 | 谁写的 | 说明 |
| --- | --- | --- |
| `video_source.mp4` | import 阶段 / `ctx.video_source_path` | 原视频（所有链的根） |
| `audio_source.wav` | import 阶段 / `ctx.audio_source_path` | 原音频 |
| `ctx.json` | 引擎（`set_step` / `set_workflow`） | 状态投影，**所有 step 都读写它** |

> `ctx.json` 是全局共享的：每个 step 进来 `read_ctx`、改自己的状态、写回。
> **两个 step 并行会互相覆盖**——这是并行化的前置障碍（见文末）。

---

## 各 step 契约

### `separate`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `ctx.audio_source_path`（**真正喂给 demucs 的输入**） |
| **检查存在** | `ctx.video_source_path`（**只 `exists()` 一次，之后不使用**） |
| **读** | `data/bin/demucs-burn-*`、`data/models/demucs/htdemucs_ft.safetensors`（缺则下载） |
| **写** | `separate/target_{0_drums,1_bass,2_other,3_vocals}.wav`、进度流 |

> ⚠️ **`separate` 不消费视频**。`separate/mod.rs:280-286` 拿到 `video_path` 后
> 只做 `exists()` 检查，随后全程使用 `audio_source_path`；`run_demucs` 的 argv 是
> `demucs-burn-* <audio> <outdir>`，**没有视频**。
>
> 也就是说 `separate` 对 `video_source.mp4` 的依赖是**纯附加的**：
> 视频丢了但音频还在时，它会报 "video_source.mp4 not found" 而不是照常工作。
> 如果这不是有意的（比如 TS 侧的遗留习惯），这行检查可以去掉，
> `separate` 的真实上游就只剩 `audio_source.wav`。

### `separate_after`

| | 路径 |
| --- | --- |
| **读** | `separate/target_{0,1,2,3}_*.wav`（stems） |
| **写** | `separate_after/target_bgm.wav`、`target_3_vocals_mixed.wav`、`target_3_vocals_gated.wav` |

**消费方**（这是最容易看漏的一处）：

- `target_bgm.wav` → **`mix_video`**（作为 BGM，做 sidechain 压低）
- `target_3_vocals_{mixed,gated}.wav` → **`asr`**（`useSeparated` 时优先用）

### `asr`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `ctx.video_source_path` |
| **读** | **仅当 `useSeparated: true`**：`separate_after/target_3_vocals_gated.wav` → 没有则 `..._mixed.wav` → 都没有则回退 |
| **读** | `separate/target_3_vocals.wav`（`vocalAudioPath` 可覆盖；上面都不命中时的最终音源） |
| **读** | `data/models/whisper/*`（缺则下载） |
| **写** | `asr/asr.json` |

### `asr_fix`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `asr/asr.json` |
| **读** | `subtitle_file_path(ctx)`（见下方「焦点文件」） |
| **写** | `subtitle_file_path(ctx)`（就地改写，或写 `asr_fix/`） |

### `sf_ocr_pre`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `ctx.video_source_path` |
| **读** | `data/bin/subtitle-finder`（缺则下载） |
| **写** | `sf_ocr_pre/frames/*.png`、`mask/`、`timeline.txt`、`keyframes.json` |

### `sf_ocr`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `sf_ocr_pre/frames/` |
| **读** | `data/bin/subtitle-ocr`（缺则下载） |
| **写** | `sf_ocr/frames.json` |
| **删** | `sf_ocr_pre/frames/`（**仅当 `cleanupFrames: true`；默认 false，即保留**） |

> **`cleanup_frames` 默认 false**（`args.rs:66`）——抽出的帧默认**保留**，
> 便于复查 OCR 结果。显式配 `true` 才会删。

### `sf_ocr_fix`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `sf_ocr/frames.json` |
| **读** | `ctx.video_source_path`（取分辨率兜底） |
| **读** | `data/bin/subtitle-ocr-post`（缺则下载） |
| **写** | `sf_ocr_fix/segment_filter.json`、`segment_filter_llm_fix.json`（`llmFix` 时） |
| **写** | `sf_ocr_fix/{frames_box_adjust,frames_box_filter,frames_merged,segment_adjust}.json`（`subtitle-ocr-post` 各级中间产物） |

> `subtitle-ocr-post` 的 `--stop-at filter-segment` 会留下一条流水线产物：
> `frames_box_adjust → frames_box_filter → frames_merged → segment_adjust → segment_filter`。
> 真正的**交付物是 `segment_filter*.json`**，其余是中间态（便于定位哪一级出问题）。

### `asr_ocr_pre`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `asr/asr.json` |
| **读** ⚠️ | `ctx.video_source_path`（**ffmpeg 抽帧的真实输入**：`-i <video>`） |
| **写** | `asr_ocr_pre/asr_split.json`、`asr_ocr_pre/frames/*.jpg` |

### `asr_ocr`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `asr_ocr_pre/frames/` |
| **写** | `asr_ocr/frames.json` |

### `asr_ocr_fix`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `asr/asr.json`、`asr_ocr_pre/asr_split.json`、`asr_ocr/frames.json` |
| **读** | `ctx.video_source_path`（分辨率） |
| **写** | `asr_ocr_fix/`（融合结果）、`subtitle_file_path(ctx)` |

### `translate`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `subtitle_file_path(ctx)` |
| **读** | `data/bin/yt-dlp` 产物（可选元信息） |
| **写** | `translate/translation.{lang}.json`、中间态 `translation.{lang}.partial.json` |

### `split_audio`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `subtitle_file_path(ctx)`（时间轴） |
| **读** | `translate/translation.{lang}.json`（`translate.enabled` 时；否则用原文） |
| **读** | `separate/target_3_vocals.wav`（dub 模式切 wav 块；subtitle 模式不需要） |
| **写** | `split_audio/split_audio.json`、`timings.json`、`vocals/*.wav` |

> **注意 `vocals_path` 指向 `separate/`，不是 `separate_after/`**——
> 这里用的是**原始**人声，不是混音后的。

### `tts`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `split_audio/split_audio.json`（逐段文本） |
| **读** | `split_audio/vocals/*.wav`（参考资料音，`MIN_REF_BYTES` 门槛） |
| **写** | `tts/wavs/*.wav`、`tts/tts.json` |
| **写** | `tts/doubled/`（参考音拼接中间态） |

### `mix_audio`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `tts/tts.json`、`tts/wavs/*.wav` |
| **读** ⚠️ | `split_audio/timings.json` |
| **写** | `mix_audio/audio_dubbing.wav`、`mix_audio/timings.json` |
| **写** | `mix_audio/{stretched,silences}/`（中间态） |

### `mix_video`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `ctx.video_source_path` |
| **读** | `mix_audio/audio_dubbing.wav`（**dub 模式**；`subtitle` 模式不需要） |
| **读** | `separate_after/target_bgm.wav` 或 `mix_video.bgmPath` 覆盖（**dub 模式**） |
| **读** | `subtitle_file_path(ctx)` 或 `mix_video.subtitlePath` 覆盖 |
| **读** | `translate/translation.{lang}.json`（烧译文字幕时） |
| **写** | `mix_video/{dub,subtitle,dub_ntl}/{id}.mp4`（最终产物） |

---

## 焦点文件：`subtitle_file_path(ctx)`

这是**唯一一个「谁写的」取决于 `subtitleSource` 的路径**，所以单列
（实现见 `utils/mod.rs:523`）：

| `subtitleSource` | 路径 | 谁写 |
| --- | --- | --- |
| `asr`（默认） | `asr_fix/asr_fix.json` | `asr_fix` |
| `sf_ocr` | `sf_ocr_fix/segment_filter.json`<br>或 `segment_filter_llm_fix.json`（`sf_ocr_fix.llmFix: true`） | `sf_ocr_fix` |
| `asr_ocr` | `asr_ocr_fix/asr_ocr_fused.json`<br>或 `asr_ocr_fused_llm_fix.json`（`asr_ocr_fix.llmFix: true`） | `asr_ocr_fix` |

> ⚠️ 注意 `asr` 源走的是 **`asr_fix/asr_fix.json`**，不是 `asr/asr.json`。
> `asr/asr.json` 是 `asr` 的**中间产物**，只被 `asr_fix` 和 `asr_ocr_pre` 读。

`translate`、`split_audio`、`mix_video` 都读它——所以**这三个 step 的上游不是固定的**，
随 `subtitleSource` 与对应 `llmFix` 开关变（路径因此有 3×2 种可能）。

---

## 由契约推导的依赖图

把上面的「读」反向连到「写」的产出，得到（按 `subtitleSource` 分）：

```
shared:  video_source.mp4 ─┬───────────────────────────────────────┐
                            │                                       │
separate ──► separate_after ┤                                       │
                            │                                       │
   ┌────────────────────────┴─────────────────────┐                 │
   │ sf_ocr 链（subtitleSource=sf_ocr）           │ asr 链（=asr）  │
   │ sf_ocr_pre ─► sf_ocr ─► sf_ocr_fix           │ asr ─► asr_fix  │
   └────────────────────────┬─────────────────────┘                 │
                            │                                       │
                            ▼                                       │
                    subtitle_file_path ◄──────────────────────────┘
                            │
                            ├──► translate ──► translation.{lang}.json ─┐
                            │                                           │
                            ▼                                           │
                      split_audio ──► split_audio/timings.json         │
                            │                │                          │
                            │                ▼                          │
                            │              tts ──► tts/tts.json ──► mix_audio
                            │                          │                │
                            │                          └────────────────┤
                            │                                           │
                            └───────────────► mix_video ◄───────────────┘
                                                ▲
                              separate_after/target_bgm.wav
```

**可并行的边**（仅两处，且都不长）：

| 并行对 | 条件 | 收益 |
| --- | --- | --- |
| `separate_after` 与 `sf_ocr_pre → sf_ocr_fix` | 两者都只依赖两个输入端 | `separate_after` 的产物要到 `mix_video` 才被读，中间一直闲置 |
| `tts` 与 `translate` 之后的字幕链 | `tts` 只读 `split_audio/` | 收益小（都在 translate 下游） |

---

## 改代码时的自检清单

改任何一个 step 之前，回答这几个问题：

1. **我改的是「写」还是「读」？** 改「写」要检查所有读它的地方（用上面的表反查）。
2. **`subtitleSource` 相关吗？** 涉及 `subtitle_file_path` 就得同时考虑三种取值。
3. **我加的读是「缺失即失败」还是「有则用」？** 前者会改变失败面。
4. **有没有破坏性副作用？**（如 `sf_ocr` 的 `cleanup_frames` 可删帧）
5. **`ctx.json` 的并发假设还成立吗？**（见下）
6. **改 step 名单时改 `StepName` 枚举**——漏登记会在编译期报错，别去改
   `STEPS_LIST` / `*_STEPS` 那几处字符串（它们有测试钉住）。

## 已知限制

- **`ctx.json` 是全局共享可变状态**：每个 step 读写它，**并行 step 会互相覆盖**。
  任何并行化都要先解决这个（进程内锁 or 改用 core 的 state 模型）。
- **路径来源**：本文先扫代码（`read_to_string` / `_path()` / `_dir()` 等），
  再对照 `workfolder/师尊带我炸修真/2/` 的**真实产出目录**核对过，两者一致。
  但那次运行只覆盖一个参数组合（`dub` + `sf_ocr` + `llmFix=true`）。
- **`input.jsonc` 的配置可覆盖部分路径**（如 `mix_video.bgmPath` / `asr.vocalAudioPath`），
  这类覆盖会让实际依赖与本文不符。
- **`grep` 追不到间接层是已知风险**（已因此错过一次 `separate_after`）。
  **发现不一致时以代码为准，并回来修本文。**
