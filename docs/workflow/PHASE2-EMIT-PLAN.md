# Phase 2 实施方案：progress / message 改 emit + 投影器

> 状态：**待审方案**（2026-11-22）。本文只描述怎么做，不含已落地的代码。
> 主计划见 [INTEGRATION-PLAN.md](./INTEGRATION-PLAN.md) §Phase 2；状态快照见 [HANDOVER.md](./HANDOVER.md)。
> 用户对范围的拍板：**只改 progress / message**；status 归执行器、`<step>/out.json` 保留写盘。

## 1. 事实基础（已核对源码，不是脑补）

| 事实 | 真源 |
| --- | --- |
| `CtxView::emit(name, value)` 当前是 **no-op** | `workflow/step_ctx.rs:72-75` |
| emit 签名 `(name: impl AsRef<str>, value: serde_json::Value)` | `workflow/step_ctx.rs:35` |
| aa-workflow 的 `WorkflowEvent::Custom { ts, name, value }`（**run_id 已于 2026-11-23 删除**，commit aa-workflow `937ace8`），语义 **observability only / not persisted / 只到 publisher、不进日志** | `lib_ls/aa-workflow/packages/workflow_core/src/event.rs:226-234`；对齐细节见 aa-workflow `docs/tanstack/WorkflowEvent.md` |
| 步边界事件已由引擎定义：StepStarted / StepFinished / StepFailed（后两者 checkpoint-persisted） | `event.rs:129-155` |
| `StepState` 7 字段：status / progress / started_at / completed_at / last_message / error_message | `state/mod.rs:126-139` |
| 当前 `<step>/state.json` 由该步骤自己写（`set_step_anyhow` → `write_step_state`，read-modify-write） | `state/mod.rs:261-277` |
| 前端步骤状态走 `get_workflow_ctx` RPC，而该 RPC **已被注释暂停**（"State 形状改造中"） | `server/src/feat/workflows/mod.rs:38-48`、`fnrpc_func.rs:47-48` |

**结论**：`emit(name, value)` → `WorkflowEvent::Custom` 是天然映射（字段逐字对应），无需新事件类型。
前端 RPC 已暂停，意味着改 emit 不会立刻"断"一个在用的链路——但投影器仍要做，否则 progress/message 无人接收、等于删掉可观测性。

## 2. 三类写点的归宿（核心结论）

### A 类 → 改成 emit（本次范围）

步骤体 `set_step_anyhow(StepPatch { progress, last_message, ... })` 里的这两个字段。当前写点分布（15 步，约 28 处 set_step 调用）：

| 文件 | set_step 调用数 | 典型 progress / message |
| --- | --- | --- |
| `separate/mod.rs` | 5 | "Separating audio..." / skipped / progress 0 |
| `asr_ocr/pre.rs` | 3 | "Splitting ASR segments..." / progress 0 |
| `asr/mod.rs`、`asr/fix.rs` | 各 2 | "Transcribing..." |
| `sf_ocr/ocr.rs`、`sf_ocr/ocr_pre.rs`、`sf_ocr/ocr_fix.rs` | 2 / 2 / 1 | "查找字幕关键帧..." / "找到 N 个关键帧" |
| `asr_ocr/ocr.rs`、`asr_ocr/fix.rs` | 2 / 2 | OCR / 融合进度 |
| `translate/mod.rs` | 2 | **每 batch 的 progress**（循环内，高频） |
| `tts/mod.rs`、`split_audio/mod.rs` | 2 / 1 | 逐段合成进度 |
| `mix_audio/mod.rs`、`mix_video/mod.rs` | 1 / 1 | "Merged" / skip |

映射：
- `progress: Some(x)` → `sc.ctx().emit("progress", json!({ "step": <id>, "value": x }))`
- `last_message: Some(s)` → `sc.ctx().emit("message", json!({ "step": <id>, "text": s }))`
- 事件名词汇 `"progress"` / `"message"` 固定（换源表 §4 说"词汇不变"，接入日不改）。

### B 类 → 删除（由执行器接管，不改成 emit）

步骤体末尾的 `status: Success` + `completed_at` + `progress: 100` 的三联写点，以及开头的 `status: Running` + `started_at`：
- `run_one_step`（`pipeline.rs`）已在调用前写 Running、失败写 Failed（INTEGRATION-PLAN §5.5）；
- 成功侧应在 `run_one_step` 收到 `Ok(())` 后由**执行器**发 StepFinished；
- `started_at` / `completed_at` 用事件自带 `ts`，步骤不再写时间戳。
- `error_message` 由执行器在 StepFailed 里发，步骤只需 `return Err`。

⚠️ 这一步是 Phase 2 里"首个行为变化点"的真正含义：步骤 body 不再碰任何 state 文件。

### C 类 → 保留写盘，不碰

`<step>/out.json` 三处：asr 实测语言、translate 语言+产物路径、mix_video 成片路径。
它们是**跨步骤事实**（下游 `AsrOutput::read` / `LangPath::from_video_dir` / `MixVideoOutput::read` 真读），
emit 是 not-persisted，改成 emit 会丢持久化语义 → 必须保留。

## 3. 落地顺序（依赖链，前一步是后一步的前提）

1. **打通最小闭环**（否则 emit 是 no-op，改了等于删）：
   - `MyRunCtx::emit` 从 no-op 改为：把 `WorkflowEvent::Custom { ts, name, value }` 追加到
     事件文件 `<video_dir>/workflow-engine/<run_id>.events.jsonl`。`ts` 由 emit 侧生成，
     `name`/`value` 直接来自 emit 参数——Custom 已无 run_id 字段（aa-workflow `937ace8`），
     emit→Custom 映射逐字。
   - **一个 video_dir 对应多次 run，事件文件按 run_id 命名**：`run_id` 是每次新 run 时传入
     （aa-workflow `RunWorkflowOptions.run_id: Option<RunId>`——复用即 resume、不传则生成新 id），
     start / resume / regen 各是一次独立 run。文件名带 run_id，多次 run 各有自己的文件。
   - **文件名本身就是自增记录，不需要单独 counter 文件**（用户 2026-11-23 定）：新 run 的
     run_id = 扫 `<video_dir>/workflow-engine/` 下已有的 `*.events.jsonl` 文件名取最大 +1
     （`run-1.events.jsonl` / `run-2.events.jsonl`…）。文件存在即记录，重启可续，省掉一个
     状态文件。边界：文件被整体清理后从 `run-1` 重来——此时旧文件也不在了，不会冲突。
   - **LocalDub 当前缺 run_id 字段**：`WorkflowArgs` 只有 `action` / `queue_id` / step 参数
     （`workflow/args.rs:210-231`），没有 run_id。落地前要给 `MyRunCtx` 加 `run_id: String`
     并在执行器构造各 run 时传入——这是必须补的基础设施，见待拍板。
   - 注：现役 FsRunStore 落盘在 `<video_dir>/workflow-store/<run_id>/logs.jsonl`
     （每 run 一个目录），与本计划的 emit 事件文件是两条通道，不要混用。
2. **投影器**：纯函数 `fold(events: [WorkflowEvent], prev: ViewState) -> ViewState`，
   写 `<video_dir>/view.json`。落盘防抖收在投影器内部。
   - status 从 StepStarted/Finished/Failed 折叠；progress/message 从 Custom 折叠；
   - progress 瞬时，重建后归零（与计划一致）。
3. **执行器发步边界**：`run_one_step` 发 StepStarted → 调 `run_step` → Ok 发 StepFinished / Err 发 StepFailed。
4. **删 B 类**：步骤体的 Running/Success/Failed + 时间戳写点全部删除。
5. **改 A 类**：28 处 progress/message → emit（按 §2 表格逐文件推进）。
6. **收尾**：`set_step_anyhow` / `write_step_state` 的调用点清零后删除这两个函数（或保留为投影器内部用）；
   `state.json → view.json` 改名；前端 `get_workflow_ctx` 恢复时改读 view.json（端点当前暂停，可最后做）。

## 4. 风险与注意

- **并发写**：投影器是 `view.json` 唯一写者，结构上消除当前 `<step>/state.json` 多步并写竞争——
  这正是引入 emit 的核心收益之一，落地时要确保没有旁路再写 view.json。
- **translate 的高频 progress**：每 batch 一次 emit（BATCH=50，长视频可达数百次），
  追加写事件文件 + 投影器防抖必须扛住；不要在 emit 里同步重写 view.json。
- **零行为变化的边界**：A 类改动本身是"换载体"（state.json → events → view.json），
  但 B 类删除是行为变化；两者混在一起时，验证要看 view.json 的最终形状而非中间过程。

## 5. 待拍板（写方案时未定，实现前需定）

1. **run_id 方案已拍板：每个 video_dir 独立自增，文件名即记录**（用户 2026-11-23 定）：
   run_id 形如 `run-1` / `run-2`，同一任务每次 start / resume / regen 自增一次；
   新号 = 扫 `<video_dir>/workflow-engine/` 下已有 `*.events.jsonl` 文件名取最大 +1，
   不另设 counter 文件。**queue_id 不能用**（服务端全局队列自增，跨视频目录）。
   run_id 走 `WorkflowArgs` 可选字段传入（复用 = resume，不传 = 生成新号），执行器
   构造 `MyRunCtx { run_id, ... }`。
2. 事件文件路径已定：`<video_dir>/workflow-engine/<run_id>.events.jsonl`（用户 2026-11-23 定）。
3. 投影器的 `ViewState` 形状：直接复用 `StepState`（7 字段）还是精简版？
4. `emit` 是否要保留同步 fallback（比如事件文件写失败时退回写 state.json）——
   按"不留兼容层"原则建议不保留，失败就 error。

## 6. 验证方式

- 只做 `cargo check --workspace --tests`（按用户偏好，不跑 test / pipeline）。
- 每个步骤改完后比对：同一输入下，改前 `<step>/state.json` 的 progress/message 与改后 `view.json` 的对应字段语义一致。
- B 类删除后确认：`<step>/state.json` 不再被步骤体创建。
