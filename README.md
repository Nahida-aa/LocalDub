# LocalDub

视频配音 / 字幕流水线：`import → separate → asr / sf_ocr → translate → tts → mix_video`，
本地工具链（ffmpeg / whisper / demucs / 关键帧 OCR）+ 云端服务（LLM 翻译、VoxCPM TTS）。
Rust 工作区（`packages/*`，`ld-core` 为 Rust 侧核心）+ TS 侧（`packages/app` Tauri UI、
`packages/cli` 入口）。开发规约见 `AGENTS.md`。

## 进行中：实验性引入 aa_workflow_core

**状态：实验性引入，引擎路径整体暂停编译（2026-09-30 起）。**

### 是什么

[aa-workflow](https://github.com/Nahida-aa/aa_workflow)（独立仓库，本地在
`lib_ls/aa-workflow`）是一个 Rust 耐久执行（durable execution）引擎，对标 TanStack
Workflow 的 **core engine** 层：headless replay 引擎 + `RunStore` 契约。内核是
**代码即 DAG**——没有声明式图，编排就是普通 async 代码，DAG 是 handler 重放时
按 `step_id` 短路 checkpoint 的 per-run 涌现产物。

LocalDub 想要的是它的耐久能力：sleep / approve / 事件等待 / 崩溃重放，以及
append-only 事件日志（`events.jsonl`）作为运行真源。背景调研见
`docs/workflow/RESEARCH.md`。

### 怎么引的

- **git 源**，rev 收敛在根 `Cargo.toml` 的 `[workspace.dependencies]` 单处维护：

  ```toml
  aa_workflow_core = { git = "https://github.com/Nahida-aa/aa_workflow.git", rev = "7009984c…" }
  ```

  `packages/core` 只写 `aa_workflow_core.workspace = true`。
  （此前是 path dep 指向 `../../../../lib_ls/aa-workflow/packages/workflow_core`，
  workspace 外本地路径克隆/CI 不可复现，故切 git 源。）
- aa-workflow 尚无语义化 tag，升降级 = `git ls-remote` 拿新 rev 后手动改根
  `Cargo.toml` 那一行。
- **当前没有任何活跃代码编译依赖它**：`packages/core/src/integration/workflow/`
  模块整体注释（为后续「声明式 DAG / 命令式 DAG」改造让路），依赖本身是恢复预留。
  本地 `lib_ls/aa-workflow` 领先 origin/main 3 个提交（redoSet 改名
  `unsafe_invalidate` → `redo_set`），LocalDub 侧无引用，切 git 源不受影响。

### 三条执行路径

`workflow` 的 start/continue 按 `ExecPath` 三选一（`engine` 与 `parallel` 同设报错）：

| 路径       | 开关                                    | 运行真源                     | 状态             |
| ---------- | --------------------------------------- | ---------------------------- | ---------------- |
| `Engine`   | `--engine` / `workflow.engine`          | `events.jsonl`（事件日志）   | 开发中，暂不可用 |
| `Parallel` | `--parallel` / `workflow.parallel`      | 各 `<step>/state.json`       | 实验性           |
| `Serial`   | 默认                                    | `state.json`                 | 稳定             |

`Engine` 臂现在打印「开发中」并报错；并行验证成熟后将成为默认行为，届时
`parallel` 标志退役。注意 `state.json` 是**投影**（给人/UI 看），不是引擎真源——
引擎 resume / continue_from / target_step 全依据事件日志。

### 恢复引擎路径

1. 取消 `packages/core/src/integration/workflow/mod.rs` 里各 `pub mod` 的注释；
2. 还原 `cmd/workflows/{start,continue_workflow}.rs` 的 `ExecPath::Engine` 臂
   （原接线以注释形式保留在分支里）；
3. 还原 `continue_workflow.rs` 里一并注释的引擎接线测试；
4. 按需同步 aa-workflow rev（本地仓领先 origin 的提交如需先合入上游）。

### 已知边界

- aa-workflow 对标范围**不含** TanStack 的 `workflow-runtime` 层（lease / sweep /
  timer 索引 / schedules）；LocalDub 侧驱动仍走 `FsRunStore` 直驱 core，
  runtime 层等收益出现再切。
- `state.json` 可丢弃、可重建（计划严格由日志派生）；与 aa-workflow 引擎的
  `ctx.state`（handler 执行期内存，不落盘）不是一回事，勿混淆。
