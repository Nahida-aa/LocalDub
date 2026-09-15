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
| **写** | `asr_fix/asr_fix.json`（详细产物，含 words/confidence 等） |
| **写** | `asr_fix/srt.json`（**规范化字幕**，见「规范化字幕」节） |

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
| **读** | `data/bin/subtitle-ocr-post`（缺则下载） |
| **写** | `sf_ocr_fix/segment_filter.json`、`segment_filter_llm_fix.json`（`llmFix` 时） |
| **写** | `sf_ocr_fix/{frames_box_adjust,frames_box_filter,frames_merged,segment_adjust}.json`（`subtitle-ocr-post` 各级中间产物） |
| **写** | `sf_ocr_fix/srt.json`（**规范化字幕**，见「规范化字幕」节） |

> 画面高度**不再从这里取**：`subtitle-ocr` v0.1.1 起把 `meta.video_height` 写进
> `frames.json`，`subtitle-ocr-post` 拿到 `--frames` 后自己读。以前为了兜底会
> `probe_video_resolution(video_source_path)` 并传 `--video-height`，现在已删。
> 实测产物：`sf_ocr/frames.json` 的 `meta` = `{"engine":"ort-rust","device":"cpu","video_height":720}`。
>
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
| **读** | `ctx.video_source_path`：**resample 抽帧**（`-i <video>`）；高度探测仅老产物回退（见下） |
| **写** | `asr_ocr_fix/asr_ocr_fused[_llm_fix].json`（详细产物） |
| **写** | `asr_ocr_fix/srt.json`（**规范化字幕**，见「规范化字幕」节） |
| **写** | `asr_ocr_fix/{frames_box_adjust,frames_box_filter,frames_merged,segment_adjust}.json`（中间产物） |

> 高度（`adjust-segment` 的 Y 惩罚归一化分母）的来源，**两条路径不同**：
>
> - `sf_ocr_fix` 走 **CLI 二进制**（Release 下载的 v0.1.1+）→ post 自己从
>   frames.json 的 `meta.video_height` 读，我们不用传（已删 `--video-height`）
> - `asr_ocr_fix` 走 **in-process Rust 库**（`subtitle_ocr_post` crate，
>   rev `d7d63c5`）→ `ocr_segment_adjust` 签名要 `video_height: f32`，
>   优先读 `frames.json` 的 `meta.video_height`（ocr-types 同 rev 起有该字段，
>   `Option<u32>`）；**仅老产物（v0.1.0-）缺字段时**回退 ffprobe 视频
>
> 高度的用途：把字幕框 Y 像素偏差归一化成 0..1 的位置惩罚（画面中部的 OCR
> 误识别压低置信度）。**传 0 不会报错，只会让位置惩罚静默失效**。

### `translate`

**恒执行**（`get_steps` 不再按 enabled 剔除）。`translate.enabled=false` 时走
**直通**：不调 LLM，`dst` 填原文，照常写 `{lang}.json`——下游步骤
从此只认这一种字幕来源（split_audio / mix_video 的「无译文用原文」分支成为
历史，可后续清理）。

> **直通时 `{lang} = 源语言`，无条件。**
>
> 没有翻译就没有「目标语言」这个概念——dst 装的是原文，产物就该以原文语言命名。
> 显式配了 `targetLang` 也一样被源语言覆盖：那在有翻译时才是"事实"，直通时只是
> "以后打算翻译成什么"的意向，不是本次事实。
>
> 为什么必须这样：`{lang}` 除了命名还驱动 `default_font`（`utils/mod.rs:355`，
> 非 `zh` → `Arial`）。若直通时 `{lang}` 是推断出的 `en`，中文字幕会被配上
> Arial——Arial 无中文字形。这个 bug 与内容语言无关，纯粹是槽位标识惹的。
>
> 规则实现在 `resolve_language`（`utils/mod.rs:607`），由
> `passthrough_targets_source_language` / `passthrough_overrides_explicit_target_lang`
> 守护。

| | 路径 |
| --- | --- |
| **读** ⚠️ | 规范化字幕 `srt.json`（按 `subtitleSource` 选目录，见「规范化字幕」节；类型化契约 `TranslateInput::from_ctx`，未知来源**报错**） |
| **读** | `data/bin/yt-dlp` 产物（可选元信息） |
| **写** | `translate/{lang}.json`（类型化契约 `TranslateOutput::for_lang`）、中间态 `{lang}.partial.json`（仅 LLM 路径，私有状态） |
| **写** | `ctx.json` 的 `target_language`——**全仓唯一写点**（`persist_target_language`），见下 |

> 存在性语义按 enabled 分：**enabled** 时 srt.json 缺失/空 → 报错（要求翻译
> 却没有内容 = 上游问题）；**disabled** 时缺失 → 空 segments 直通（上游被
> 禁用，本来就没有字幕——与旧行为「translate 被移除」的成功语义一致）。
>
> 注意：直通时 `{lang}.json` 的旧语言版本**不会被清理**。切换语言
> 重跑会留下多份（真实数据里已有任务同时存在 `vi.json` 与
> `zh.json`）。这是合法状态——「当前是哪份」由 `ctx.target_language`
> 单一写入点决定，不靠扫目录。

> **`{lang}.json` 的路径是单点构造的。** 它是全流水线唯一「产物被 5 处
> 各拼一遍」的文件（写侧 + split_audio 读 + mix_video 读 + engine 事件登记，
> 以及两处仅为拼错误信息）。现由 `TranslateOutput` 收口：`translation_file_path`
> 只在 `TranslateOutput::for_lang` 里被调用一次，其余消费点接收该类型。
> 改产物命名不会再漏改——**但也没有第二处保留旧写法可对照**，故有
> `translate_output_matches_canonical_path` 钉死路径形状。

> **`ctx.target_language` 只由 translate 写，且只写一次。**
>
> 这是刻意的**写入权限约束**。此前写回藏在 `resolve_language` 内部
> （「算出来变了就顺手写回」），于是一个**看起来纯读**的函数带着隐形写副作用，
> 产生顺序依赖：`split_audio` 先 `read_translation_result(ctx)`（读 ctx 里的旧值）
> **再** `resolve_language(ctx)`（顺带改写 ctx.json）——读在写之前，读到的文件名
> 与它认定的语言可以不是同一个。症状是报「文件不存在」，但文件在，只是名字不同。
>
> 现在拆开：
> - `resolve_language(ctx)` —— **纯函数**，无副作用，谁都能随便调
> - `persist_target_language(video_dir, lang)` —— 唯一写点，
>   **只由 `step_translate` 调用一次**；失败返回 `Err`（原先只 `warn`）
>
> 由 `resolve_language_is_pure_and_does_not_persist` 守护：它调完
> `resolve_language` 后断言磁盘上的 ctx.json **没被改动**。把写回塞回去这条会红
> （已实测）。
>
> #### 已完成：下游改为「读事实」
>
> `resolve_language` 现在**只剩 translate 一个调用者**。其余各步骤的取法：
>
> | 调用点 | 原来 | 现在 |
> | --- | --- | --- |
> | `translate/mod.rs:190` | `resolve_language` | **保留**（它就是决策者） |
> | `split_audio/mod.rs` | `resolve_language` | `LangPath::from_ctx`（路径+语言）；源语言用 `src_language` |
> | `mix_video/mod.rs:63` | `resolve_language` | `LangPath::from_ctx(...).lang()` |
> | `workflows/engine.rs:89` | `resolve_language` | `LangPath::from_ctx` |
> | `asr/fix.rs:39` | `resolve_language` | `src_language(ctx)`（它只要源语言） |
>
> 为什么必须改：`resolve_language` 是**现算**，`ctx.target_language` 是**已落定
> 事实**。两者曾**恰好**一致（纯函数 + 同输入），但那是巧合的稳定：
> 推断逻辑一改（如直通时目标=源），下游现算的结果就变，而
> `ctx.target_language` 是 translate 当时落盘的 → 中途重跑下游步骤即分叉。
> 服务端还有 `setCtx` 合并路径（`server/src/feat/workflows/mod.rs:85`），
> input 可能在步骤之间被改。
>
> 现在两条路各有明确用途：
> - **`LangPath::new(dir, lang)`** —— 翻译步骤专用（它有权决定 `lang`）
> - **`LangPath::from_ctx(ctx)`** —— 后续步骤专用（读事实，缺失即报错）
>
> 由 `target_lang_comes_from_persisted_fact_not_config` 守护：构造
> 「配置说 vi、事实是 zh」的错配，断言 `mix_video` 取 **zh**，并连带验证
> 字体选到 CJK 而非 Arial。改回现算这条会红（已实测）。
>
> **源语言单独走 [`src_language`]**：`ctx.asr_language`（ASR 实测）>
> `input.workflow.sourceLang` > 兜底。纯 OCR / subtitle 路径没有 ASR，
> 必须靠后两级回退——所以它**不能**简化成读 `ctx.asr_language`。
>
> 相关：`TranslateOutput::from_ctx` 只取 `ctx.target_language`、不调
> `resolve_language`——保证「读的」与「报的」是同一路径。多语言共存是合法状态
> （真实数据里已有任务同时存在 `vi.json` 与 `zh.json`），
> 所以不能用「扫产物目录」来判定当前语言，必须靠这个单一写入点。

### `split_audio`

| | 路径 |
| --- | --- |
| **读** ⚠️ | `translate/{lang}.json`——**唯一字幕来源**（时间轴 + 文本 + dst；直通时 dst=原文） |
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
| **读** ⚠️ | `ctx.video_source_path`（ffmpeg 输入） |
| **读** | 字幕轨按 **pipeline** 分（见下） |
| **读** | `mix_audio/audio_dubbing.wav`（**仅 dub**） |
| **读** | `separate_after/target_bgm.wav` 或 `mix_video.bgmPath` 覆盖（**仅 dub**） |
| **写** | `mix_video/{dub,subtitle,dub_ntl}/{id}.mp4`（最终产物）、中间 SRT |

**字幕轨来源（按 pipeline 分，代码 `mix_video/mod.rs:97`）**：

| pipeline | 来源 | 说明 |
| --- | --- | --- |
| **dub** | `mix_audio/timings.json`（`mix_video/mod.rs:366`） | 用 `actual_start/actual_end`（mix_audio 修正后的**实际**音频位置）生成 SRT——烧进去的字幕才能和配音对齐 |
| **subtitle** | 两选一（`mix_video/mod.rs:295`）：`split_audio/split_audio.json`（`vadAlign: true`）／`translate/{lang}.json`（默认；`mix_video.srtPath` 可覆盖为外部 SRT 文本文件） | 无配音，直接按字幕时序烧 |

> 曾把 subtitle 分支的三选一误当成 mix_video 的全部来源，漏了 dub 模式这条
> 最关键的边：**mix_audio 的 timings 产物是 mix_video 的字幕轨输入**。
> 翻译恒执行后，subtitle 分支的「焦点文件」路径已删——顺带修掉一个**潜伏
> bug**：旧实现无 `srtPath` 覆盖时把焦点文件 JSON 塞给 SRT 文本解析器
> （`read_srt_file_to_segs`），块数 < 3 全被跳过 → 空字幕，静默。

---

## 规范化字幕 `srt.json`（现役契约）；焦点文件（已退役）

### 现役契约：`{fix_dir}/srt.json`

三个 fix 步骤（`asr_fix` / `sf_ocr_fix` / `asr_ocr_fix`）收尾时各自落一个**规范化
字幕**：`{"result": {"segments": [SubtitleSegment]}}`，段结构用 `subtitle` crate
的权威类型（`text` / `start_ms` / `end_ms`，详细字段留在各自的详细产物里）。

**要点**：

- **`llmFix` 维度对消费者不可见**——跑没跑 LLM 修正由产出方消化
  （详细文件照写，最后统一落 srt.json），下游只认 srt.json。
- **路径由构造函数唯一产生**：`paths::{asr,sf_ocr,asr_ocr}_fix::SrtPath::new(video_dir)`。
  字段私有，故 `"srt.json"` 与各自目录名**在全仓只拼一次**——此前散在 6 处
  （3 个产出方 `out_dir.join("srt.json")` + 消费者手拼 + engine 事件登记）。
  三者共同实现 `paths::SrtJsonPath`（`new` / `as_path` / `dir`）。
- 三种 `SrtPath` 是不同类型、不可互换，`from_ctx` 的 match 必须逐支写
  （or-pattern 会因类型不同而编译失败——这是刻意的）。
- 消费方由类型保证：`TranslateInput::from_ctx`（`translate/mod.rs`）按
  `subtitleSource` 解析到 `TranslateInput::Asr / SfOcr / AsrOcr` 三变体，
  未知来源报错；srt.json 缺失的存在性语义由 `translate.enabled` 决定
  （见 `translate` 节）。
- **迁移已完成**：translate / split_audio / mix_video 三个消费者全部只认
  srt.json 与 {lang}.json，不再按 `subtitleSource`/`llmFix` 找字幕。

### 已退役：焦点文件（`subtitle_file_path(ctx)`）

旧契约：按 `subtitleSource` × `llmFix`（3×2）解析出「权威字幕文件」：

| `subtitleSource` | 路径 |
| --- | --- |
| `asr`（默认） | `asr_fix/asr_fix.json` |
| `sf_ocr` | `sf_ocr_fix/segment_filter.json` 或 `segment_filter_llm_fix.json` |
| `asr_ocr` | `asr_ocr_fix/asr_ocr_fused.json` 或 `asr_ocr_fused_llm_fix.json` |

**2026-09-14 退役**：translate 恒执行 + 直通模式后，三个消费者（translate /
split_audio / mix_video noTranslate）全部改道，helper 已删除。留档原因：

- `asr` 源走的是 `asr_fix/asr_fix.json`（**asr_fix 的输出**），不是 `asr/asr.json`
  ——`asr/asr.json` 只是中间产物，仅被 `asr_fix` 和 `asr_ocr_pre` 读。
- 这些详细文件**仍是各 fix 步骤的内部产物**（含 confidence/words 等下游
  不需要的字段），只是不再作为下游契约。
- 曾有一个潜伏 bug 借此藏身：mix_video noTranslate 分支把焦点文件 JSON
  塞给 SRT 文本解析器 → 空字幕（见 `mix_video` 节）。

---

## 由契约推导的依赖图

把上面的「读」反向连到「写」的产出（**translate 恒执行**后）：

```
shared:  video_source.mp4 ─┬───────────────────────────────────────┐
                            │                                       │
separate ──► separate_after ┤                                       │
                            │                                       │
   ┌────────────────────────┴─────────────────────┐                 │
   │ sf_ocr 链（subtitleSource=sf_ocr）           │ asr 链（=asr）  │
   │ sf_ocr_pre ─► sf_ocr ─► sf_ocr_fix ──┐       │ asr ─► asr_fix ─┤
   └──────────────────────────────────────┼───────┘─────────────────┘
                                          ▼
                                    srt.json（各链规范化产物）
                                          │
                                          ▼
                                  translate ──► {lang}.json
                                          │  （enabled=false: dst=原文）
                                          ▼
                                    split_audio ──► timings.json
                                          │               │
                                          ▼               ▼
                                        tts ──► tts.json ──► mix_audio
                                  (vadAlign:            │
                                   split_audio.json)    │ audio_dubbing.wav
                                                        ▼ + timings.json
                                            mix_video ◄───────┘
                                              ▲
                              separate_after/target_bgm.wav

subtitle pipeline 的 mix_video 不吃 mix_audio 产物，
字幕轨走 split_audio.json（vadAlign）或 {lang}.json（其余）。
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

## 有护栏的部分 / 没有的部分

`steps::utils::steps` 里有三条护栏测试，**把一部分「会悄悄过期」变成了「过期就红」**：

| 护栏 | 能抓 | 抓不到 |
| --- | --- | --- |
| `all_pipeline_constants_are_valid_step_names` | pipeline 常量里出现非法的 step 名 | — |
| `steps_list_matches_all_steps` | `STEPS_LIST` 与枚举漂移 | — |
| `dependencies_doc_covers_every_step` | **本文档**缺某个 step 的小节、小节名与枚举不符、某节缺「读」或「写」 | **路径内容对不对** |

**关键边界**：最后一条只校验**覆盖面**（有没有写），**不校验内容对不对**。
它拦得住「新增 step 忘了写文档」「step 改名文档没跟」，但拦不住
「路径写在文档里、代码已经改了」——那需要真正解析路径，而那会撞上
「跨 helper 的间接层」（`bgm_path()` 那类），目前不做。

所以 **`DEPENDENCIES.md` 的路径仍然可能过期**，只是「少一个 step」这类
粗漏不会再发生。发现路径与代码不符时，仍以代码为准并回来修本文。
