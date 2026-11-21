# sf_ocr_dub 接入 aa-workflow 引擎——过渡改造计划

> 状态: 计划 v0.1（2026-11-21）。
> 背景: `parallel.rs` 是**过渡方案**（自研执行器，无事件日志 / replay / pause / approve）；
> 引擎接入曾因 aa-workflow API 与设计思想未定型而整体下线（`f437750e`）。
> 现在库已趋稳（DagSpec 生成 handler / `sc` 双快照 / publisher），重新评估接入。
> 本文角色: 接入前的改造计划 + Phase 0 调查记录。**以源码为准**——
> `docs/DEPENDENCIES.md` 部分过时（from_state/from_video_dir 已演化为
> `XxxInput::new(video_dir, &Input)`），不可作为依据。

## 0. 旧接入的错误解剖（不再重犯）

1. **lib API 与设计思想未定型就接入**——RunStore → WorkflowExecutionStore 契约迁移、
   RunState 字段连续变动（`0d01306b`）等迫使下线；`parallel.rs` 是这段时期的过渡产物。
2. **state.json 混装两种生命周期**——import 诞生的输入事实（身份 / url / title /
   根事实）与 run 期间变更的运行投影同住一个 RMW 文件，步骤执行期间也要读写 →
   并行互踩 → `1f11f185` 拆分（`init.json` + `<step>/state.json`，单写者原则）。
3. **`initialize` 读 state.json**——run 的种子去读一个 run 之前诞生的混合文件。

**归属原则（写入时机决定归属）**：

- 诞生早于 run 的事实 → **run input**（`start_run` 时冻结）
- run 期间变更的投影 → **事件流的下游**（引擎事件日志为真源，投影器折叠）
- 跨步骤数据 → **盘上产物**（现役）/ **checkpoint**（接入后）

## 1. 现状事实（2026-11-21 源码勘察）

- 现役主链: `workflows/pipeline.rs::run_step`（15 步枚举穷尽分发，无 `_` 兜底）
  → `XxxInput::new(video_dir, &Input)` → `fn step_x(&XxxInput) -> anyhow::Result<()>`。
- 步骤返回 `()`: 产物全在盘上（约定路径，`paths` 模块唯一产生）；无返回值产物。
- state 语义: **无流程级可变业务状态**。`state.json` 是身份 + 运行投影；
  `StepState` 是单步执行投影（单写者）；跨步骤数据 100% 走盘上产物。
- 全局 `state.json` 写时序: import 诞生（身份 + 事实 + 配置）→ run 边界
  `set_workflow`（编排器专用，步骤执行期间零写入）→ 读者仅 CLI / UI。
- 单步 `<step>/state.json` 写时序: step 起点诞生（Running）→ 执行中反复 patch
  （进度流，如 `separate` 解析 demucs stdout 百分比）→ 收尾 Success / Failed；
  `<step>/out.json` 做完那一刻写一次。
- 并行: `workflows/dag.rs`（`StepDag`——步骤级声明 + 构造期 Kahn 校验；
  依赖边随配置变，每个 workflow 一张具体小图）+ `parallel.rs`（OS 线程执行器；
  失败**不取消**另一分支、全部收尾汇总；续跑**按步跳过**——读自己 `<step>/state.json`
  的 `Success`）——**过渡方案**。

## 2. 接入原则（与 aa-workflow 教义对齐）

- **对外观测走发送（emit / delta，不追加日志，丢了无害）；对内数据走返回值与
  依赖关系（进 checkpoint，重放负责）。两个通道不共用任何载体。**
- **写入时机决定归属**（见 §0）。
- **state.json ≠ ctx.state**: 接入后 `TState` 极薄（DAG 形态下 state 无写入者，
  一次 run 内恒定 ≡ `initialize` 产物）；LocalDub 的状态文件是**投影层**，
  改名以消除字面撞名（§3 Phase 1）。

## 3. 过渡期改造清单（在 LocalDub 侧；全部行为不变的前提下分阶段）

### Phase 0: 接口勘察（零行为改动）

- 调查 15 个步骤函数的接口 / 入参（字段、来源、外部调用、写点）——结果见 §5。
- 目的: 确认「步骤 = 业务本体 + emit」改造的每一处落点；核实步骤是否零改造可搬。

### Phase 1: 改名 + emit 通道 + 投影器

- `state.json` → `view.json`（各步 `state.json` → 投影器产物）——消除与
  ctx.state 的字面撞名。
- 本地 `emit(name, value)` 通道（事件形状用 aa-workflow 的 `WorkflowEvent`——
  LocalDub 已依赖 `aa_workflow_core` git 源）+ 投影器（`fold(events) → view` 文件，
  落盘防抖收在投影器内部）。
- 职责边界对齐引擎: **步边界事件（Running / Success / Failed）由执行器发**；
  **步骤只发 progress / custom**（与引擎「STEP 事件在耐久边界由引擎发」同构）。
- `set_step` / `set_workflow` / 直写文件的调用点全部退役——步骤不再碰任何
  state 文件；`view` 文件唯一写者 = 投影器（并发写问题结构上消失）。
- 崩溃一致性: 投影器是纯函数 `fold(events) → view`，可从事件流重建
  （status 可重建；progress 瞬时，重建后归零）。

### Phase 2: 接入（换源表见 §4）

- parallel.rs / StepDag 退役（`DagShape` + 引擎 dataflow 调度接管）；
- 步骤闭包搬进 DAG 节点闭包（`spawn_blocking` 包装阻塞子进程编排）。

## 4. 接入日换源表

| 过渡期 | 接入日 | 改动量 |
| --- | --- | --- |
| 本地执行器发步边界事件 | 引擎原生 STEP 事件（追加日志） | 投影器事件源切换 |
| 步骤 `emit("progress", …)`（本地函数） | `sc.ctx.emit(...)` | 函数签名；词汇不变 |
| 步骤闭包 = 业务 + emit | 原样搬进 DAG 节点闭包 | 包 `spawn_blocking` |
| 投影器（输入 `WorkflowEvent`） | 不改 | **零** |
| `view.json` / `init.json` / UI | 不改 | **零** |
| `StepDag` + `parallel.rs` | 退役 | `DagShape`（dag_policies）+ 引擎调度 |

### 待拍板决策

1. **checkpoint 内容**: 产物清单 JSON（旧 `drive_step` 的 `step_artifacts` 形状）
   或 Null（短路只看 checkpoint 存在性）。
2. **失败语义**: 引擎调度器 fail-fast（`abort_all`）vs LocalDub 裁决
   （一分支失败不取消、全部收尾汇总）——保持裁决需调度器加选项。
3. **shape 的配置依赖**: translate 上游随 `subtitle_source` 变、mix_video 随
   `output_mode` 变——`DagShape` 用 union graph 是否可接受（dub 的下游闭包
   与 config 无关，初步可行）。
4. **store**: FileExecutionStore（aa-workflow 契约）vs LocalDub 自有 FsRunStore。

## 5. Phase 0 调查记录（2026-11-21，全部源码核实）

### 5.1 分发层

- `run_pipeline(input)` — `pipeline.rs:48`：每步循环内**重读 input.json**（:86）；
  编排器写 `state.json` 的 workflow 子树（running :72 / failed :104 / success :117）。
- `run_step(step, video_dir, input)` — `pipeline.rs:138-161`：枚举穷尽 match，
  每分支 `step_xxx(&XxxInput::new(video_dir, input))`。
- `run_one_step` — `pipeline.rs:169-207`：set_step(Running) → run_step → 失败
  set_step(Failed)。parallel.rs 走同一入口。
- 步骤**不写**顶层 `state.json` / `input.json`（`state/mod.rs:18-23` 明文约束）。

### 5.2 逐步骤接口（字段 → 来源）

记法：字段(来源)；来源 ∈ 身份 / args(`input.steps.*`) / wf(`input.workflow.*`) /
约定(paths 模块拼法) / 解析(运行时推导) / 事实(`<step>/out.json` 落定事实)。

| step | 签名位置 | 字段 | 外部调用 | 写点 | set_step |
| --- | --- | --- | --- | --- | --- |
| separate | separate/mod.rs:221 | video_dir(身份)、audio_source_path(约定)、args(SeparateArgs)、output_mode(wf) | **demucs 子进程**（:97，stdout 百分比流）+ ensure_bin + 模型校验 | separate/target_{i}_{stem}.wav | ×5（:168,184 进度 / :230 skip / :244,331） |
| separate_after | separate/after.rs:16 | video_dir、vocals_path(约定)、stems_dir(约定)、args(**=AsrArgs** 跨步借用) | ffmpeg ×4（amix/混音/agate） | separate_after/{target_bgm,target_3_vocals_mixed,gated}.wav | ×2（:20,160） |
| sf_ocr_pre | sf_ocr/ocr_pre.rs:17 | video_dir、video_source_path(约定) | **subtitle-finder 子进程** + ensure_bin | sf_ocr_pre/frames/、mask/、timeline.txt、keyframes.json | ×2（:21,97） |
| sf_ocr | sf_ocr/ocr.rs:16 | video_dir、keyframes_dir(约定)、args(SfOcrArgs) | **subtitle-ocr 子进程** + ensure_bin | sf_ocr/frames.json（cleanup 时删帧目录） | ×2（:20,109） |
| sf_ocr_fix | sf_ocr/ocr_fix.rs:18 | video_dir、frames_path(约定)、source_lang(解析：`<asr>/out.json`)、args(OcrFixArgs) | **subtitle-ocr-post 子进程** + `llm::ocr_llm_fix`（降级保留原文） | sf_ocr_fix/{segment_filter,segment_filter_llm_fix,srt}.json | ×1（:146） |
| translate | translate/mod.rs:178 | srt_json_path(约定，subtitle_source 三选一)、video_dir、args(TranslateArgs)、src_lang(解析)、target_lang(解析) | **LLM ×2**（preprocess :114 仅当 ytdlp_info 存在；逐 batch :136，BATCH=50） | translate/{lang}.json、partial（完成即删）、**out.json**（全仓唯一写者） | ×2（:340 每 batch / :393） |
| split_audio | split_audio/mod.rs:47 | video_dir、audio_path(约定+存在性仲裁)、translate_path(**事实：translate/out.json**)、args、source_lang(解析)、is_dub(wf) | ffprobe + ffmpeg 逐段切块 + 可选 vad | split_audio/vocals/{:04}.wav、split_audio.json、timings.json | ×1（:161） |
| asr | asr/mod.rs:26 | video_dir、audio_path(约定组：Custom/Separated/Raw 三选一)、args(AsrArgs)、source_lang(wf，None→auto) | ffmpeg 转码 + **whisper-vulkan 子进程**（runtime/device 硬编码 ggml/vulkan）+ astats | asr/asr.json、原始 JSON 归档、whisper-input.wav、**out.json**（实测语言） | ×2（:30,332） |
| asr_fix | asr/fix.rs:13 | video_dir、asr_path(约定)、source_lang(解析)、args(AsrFixArgs+LlmFixArgs) | `llm::chat_completions`（失败保留原文） | asr_fix/{asr_fix,srt}.json | ×2（:61,138） |
| asr_ocr_pre | asr_ocr/pre.rs:136 | video_dir、asr_path(约定)、video_source_path(约定)、args(仅 fps 字段，**体未消费**) | ffmpeg 逐时间点抽帧 | asr_ocr_pre/asr_split.json、frames/{:07}.jpg | ×3（:140,213,292） |
| asr_ocr | asr_ocr/ocr.rs:17 | video_dir、frames_dir(约定)、args(复用 SfOcrArgs) | **subtitle-ocr 子进程** + ensure_bin | asr_ocr/frames.json（cleanup 删帧） | ×2（:21,100） |
| asr_ocr_fix | asr_ocr/fix.rs:225 | video_dir、asr_split_path(约定)、ocr_frames_path(约定)、source_lang(解析)、args(OcrFixArgs 全量+isResample)、**max_advance_ms(跨步借用 mix_audio 配置)** | in-process `subtitle_ocr_post` 库调用 + 可选重采样子进程 + `llm::ocr_llm_fix` + ffprobe | asr_ocr_fix/ 下 8-9 个 json + resampled_frames/ + srt.json | ×2（:229,495） |
| tts | tts/mod.rs:44 | video_dir、vocals_dir(约定)、timings_path(约定，typed 读)、args(TtsArgs)、is_start(wf) | cloud→**VoxCPM HTTP**；本地→voxcpm-burn-* 子进程（Retryer ×2）+ ref 音 concat + ffprobe | tts/wavs/{:04}.wav（失败写静音占位）、ref_doubled/、tts.json | ×2（:319 每段 / :444） |
| mix_audio | mix_audio/mod.rs:27 | video_dir、timings_path(约定)、wavs_dir(约定)、args(MixAudioArgs) | ffmpeg 逐段（去静音/变速 rubberband）+ concat + ffprobe | mix_audio/{stretched,silences,concat_list,audio_dubbing,timings}.json/wav | ×1（:262） |
| mix_video | mix_video/mod.rs:24 | video_dir、video_id(解析)、video_source_path(约定)、output_mode(wf)、target_lang(**事实：translate/out.json**)、no_translate(wf)、subtitle_source(wf)、args(MixVideoArgs)、branch(约定组，dub/subtitle 二选一) | **ffmpeg ×3**（烧字幕/混流/成片，300s 超时）+ ffprobe | mix_video/{lang}.srt、audio_mixed.m4a、成片 mp4、**out.json**（final_video_path） | ×2（:31 skip / :196） |

### 5.3 Input 字段来源统计（67 个字段）

| 来源 | 数量 | 备注 |
| --- | --- | --- |
| video_dir（身份入参） | 15 | 每步一个 |
| args（`input.steps.*`） | 15 | 14 个自身子树 + **1 个跨步借用**（asr_ocr_fix.max_advance_ms ← `input.steps.mix_audio`） |
| workflow 配置（`input.workflow.*`） | 7 | output_mode / is_dub / source_lang / is_start / no_translate / subtitle_source 等 |
| 约定路径 | 21 | 全部经 paths.rs newtype 构造；MixVideo.branch 一个字段含 3 条 dub 路径 |
| 解析计算 | 7 | resolve_language ×2 + src_language ×4 + video_id |
| 跨步产物事实（`<step>/out.json`） | 2 | split_audio.translate_path、mix_video.target_lang（均读 `<translate>/out.json`） |
| **ctx / state.json** | **0** | **无任何步骤 Input 读 state.json** |

### 5.4 state 耦合核实（接入关键结论）

- **没有任何步骤读顶层 `state.json`、`input.json` 或其他步骤的 state**。步骤对
  状态的唯一接触是 `set_step` 写**自己的** `<step>/state.json`（读 `read_step_state`
  的调用点全部在 `#[cfg(test)]` 内）。
- 读他步产物仅两种形态，均为**磁盘文件、非 state**：
  1. `src_language` / `resolve_language` → 读 **`<asr>/out.json`**（影响 sf_ocr_fix /
     translate / split_audio / asr_ocr_fix 的 Input 构造）；
  2. `LangPath::from_video_dir` → 读 **`<translate>/out.json`**（影响 split_audio /
     mix_video 的 Input 构造，缺失即 Err）。
- `new` 返回 `Result` 的步骤 5 个：separate / translate / split_audio / mix_audio /
  mix_video（fail-fast 校验在 Input 构造点）。
- **接入结论：步骤本体除两个 out.json 事实文件外零状态耦合——节点闭包 =
  `XxxInput::new(video_dir, &input)` 原样可搬。**

### 5.5 set_step / out.json 写点清单（Phase 1 退役清单）

- 编排器：set_workflow ×3（pipeline running/failed/success）；set_step Running/Failed
  （run_one_step）。
- 步骤侧 set_step（写自己的 `<step>/state.json`）共 **28 个调用点**，分布见 §5.2 表
  末列；其中带 `.ok()` 忽略失败的进度 patch（separate :168,184、translate :340、
  tts :319）——接入后换 `sc.ctx.emit`。
- `<step>/out.json` 写点仅 3 个：asr（out.rs:121）、translate（out.rs:153）、
  mix_video（out.rs:28）——底层 `state::write_step_out`；接入后由
  `STEP_FINISHED.result` 投影（checkpoint 为真源）。

### 5.6 外部调用汇总

- **LLM（OpenAI 兼容 HTTP）5 处**：asr_fix、translate preprocess、translate 批量、
  sf_ocr_fix、asr_ocr_fix。
- **子进程 7 类**：demucs、subtitle-finder、subtitle-ocr（×3 调用点）、
  subtitle-ocr-post、whisper-vulkan、voxcpm（本地子进程 / cloud HTTP 双 runtime）、
  ffmpeg/ffprobe（约 45 个调用点，被 8 个步骤使用）。
- **二进制就绪**：`ensure_bin` 自动下载 ×6；tts 本地后端 `find_release_bin`。

### 5.7 文档/代码冲突（死参数——接入时顺手清理）

- `SfOcrArgs.runtime/device`：步骤体未消费（不透传给二进制）；
- `OcrFixArgs` 的 iso/adjust/box/merge/dedup：sf_ocr_fix 步骤体未透传（仅 asr_ocr_fix
  的 flatten 版消费）；
- `AsrArgs.runtime/device`：被硬编码覆盖为 ggml/vulkan，枚举默认不生效；
- `TtsArgs.runtime`：serde default=Cloud 与枚举 `#[default]`=Ggml 不一致（以 serde 为准）；
- `AsrOcrPreArgs.fps`：步骤体未消费（抽帧步长硬编码）；
- `MixVideoArgs.srt_path`：步骤体未消费（自行拼 `{target_lang}.srt`）。
