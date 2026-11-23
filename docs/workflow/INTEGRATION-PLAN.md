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

- 现役主链: `workflow/pipeline.rs::run_step`（15 步枚举穷尽分发，无 `_` 兜底）
  → `XxxInput::new(video_dir, &Input)` → `fn step_x(&XxxInput) -> anyhow::Result<()>`。
- 步骤返回 `()`: 产物全在盘上（约定路径，`paths` 模块唯一产生）；无返回值产物。
- state 语义: **无流程级可变业务状态**。`state.json` 是身份 + 运行投影；
  `StepState` 是单步执行投影（单写者）；跨步骤数据 100% 走盘上产物。
- 全局 `state.json` 写时序: import 诞生（身份 + 事实 + 配置）→ run 边界
  `set_workflow`（编排器专用，步骤执行期间零写入）→ 读者仅 CLI / UI。
- 单步 `<step>/state.json` 写时序: step 起点诞生（Running）→ 执行中反复 patch
  （进度流，如 `separate` 解析 demucs stdout 百分比）→ 收尾 Success / Failed；
  `<step>/out.json` 做完那一刻写一次。
- 并行: `workflow/dag.rs`（`StepDag`——步骤级声明 + 构造期 Kahn 校验；
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

### Phase 1: 步骤签名改造（**零行为改动**）

步骤签名从单参数改为引擎预演形状：

```rust
// 现役：fn step_x(input: &XxxInput) -> Result<()>
// 目标：fn step_x(sc: impl StepCtxView, deps: XxxDeps) -> Result<()>
```

- **两个 trait = 消歧机制**（消费方端口，六边形：端口归消费方；库侧不定义
  ——构造权/单实现者/面未定三论据见 §6）：
  - **`CtxView`**：run 级视图（镜像引擎 `RunCtx` 公开面）。**`ctx` 限定符标记
    访问出处**：`sc.ctx().input()` 读作「工作流上下文的输入」——工作流 input
    **不切片、不下发、不重命名**（params 切片提案否决：那会重塑 input 的身份，
    使它看起来像步骤参数），它以工作流级事实的身份经限定路径访问；
  - **`StepCtxView`**：step 级（`ctx()` 关联类型访问器 / `id` / `attempt`）。

  ```rust
  pub trait CtxView {
      fn input(&self) -> &Input;   // 工作流输入（typed，非擦除）
      fn emit(&self, name: impl AsRef<str>, value: serde_json::Value);  // 签名同 RunCtx::emit
  }
  pub trait StepCtxView {
      type Ctx: CtxView;           // 关联类型（非 dyn）：emit 零偏离、静态分发
      fn ctx(&self) -> &Self::Ctx;
      fn id(&self) -> &str;
      fn attempt(&self) -> usize;
  }
  ```

- **接入日**：`impl CtxView for RunCtx<Input, TState>` + `impl StepCtxView for
  StepCtx<Input, TState>`（引擎字段/方法全 pub，各十行）——步骤签名与 body
  **零改动**；引擎面演化的漂移**局部化在两个 impl 块**（不散布 15 个 body）。
- **过渡实体**：`MyStepCtx`/`MyRunCtx`（本地执行器构造，实现两个 trait）；
  `state` 位与便捷方法刻意缺席（§5.4 零 state 读取；`video_dir` 走
  `ctx().input().video_dir()` 既有路径）。
- **emit 落点**：Phase 1 no-op → Phase 2 本地通道（投影器折叠 view）→ 接入日
  引擎 publisher（同一投影器换源）。

- **`XxxDeps` = 事实面拆分**：`XxxInput` 按来源拆两半——意图面（args / wf 配置 /
  video_dir，来自 run input）并入 `sc.ctx.input` 的取用；事实面（约定路径产物 /
  out.json 事实 / 依赖上游的解析）成为显式的 `XxxDeps`。**入口步骤**
  （separate / sf_ocr_pre——只依赖 import 事实）无 deps 参数。
- **解析计算拆半**：`resolve_language` 拆开——src 归 Deps 构造、target 归 sc 侧。
  ⚠️ **src 不必然依赖 `<asr>/out.json`**：真源 `SourceLang::from_video_dir`
  （`state/lang.rs:24` → `resolve_source_language:70`）是三级回退
  `<asr>/out.json` 实测 > `input.workflow.source_lang` 配置 > `unknown`（空串，
  不臆测 zh）；`asr_language()` 用 `.ok().flatten()`，读不到返回 None 不报错
  （纯 OCR / subtitle 路径没有 ASR，走后两级）。故 Deps 应表达"**可选的** ASR
  实测语言，由装配层从 `asr_language(video_dir)` 取并与配置算出 `src_lang`/`target_lang: LangStr` 注入；
  配置级 `source_lang` 仍在 sc 侧（已被 `sc.ctx().input()` 覆盖）。target 依赖
  src + `input.workflow.target_lang`，归 sc 侧现算。
- **deps 多来源 = 工作流装配层注入**：dub 模式的三种工作流
  （asr / sf_ocr / asr_ocr）各自装配——`step_translate` 定义假设不知道自己在
  哪个工作流。`TranslateDeps::from_input(&WorkflowInput)` 按 `subtitle_source` 选
  asr_fix / sf_ocr_fix / asr_ocr_fix 三来源之一读规范化字幕，缺失/空即就地
  报错；字幕段以 `srt_segs: Vec<SubtitleSegment>` 直接注入（**不再**是
  `srt_json: NormalizedSrtPath` 路径 + 步骤内 dispatch，旧 `SubtitleSourcePath`
  枚举已废弃），步骤体只读这三份算好的事实。
- **存在性仲裁上移装配层**：asr 回退链（gated>mixed>vocals）、split_audio
  （vocals→audio_source）的仲裁从 `XxxInput::new` 移到 Deps 装配点（DAG 序保证
  上游产物此时已定）；`paths.rs` 单一产生点不变（调用点搬家）。
- **StepCtxView trait 提案修订记录**：初版否决（镜像 struct）→ **改判采纳
  （消费方端口）**——决定性论据：漂移局部化（引擎面演化只修两个 impl，不散布
  15 个 body）+ 签名稳定性（接入日零签名替换）+ 端口归消费方的六边形原则
  （LocalDub 接入完成后成为 aa-workflow docs example 的真实素材）。库侧仍不
  定义（构造权/单实现者/面未定三论据不变，见 §6）。
- **「消费者自定义 view trait」的正解 = 接入后教学**：生态惯例是 context/handle
  用具体类型（tokio Handle / Temporal ctx / axum extractor），trait 只在多实现者
  是设计目的处由库定义（tower Service）；六边形原则下端口属于消费方——LocalDub
  接入完成后把真实案例写成 aa-workflow 的 docs example（外部 trait + impl for
  StepCtx + fake ctx 跑通 step 函数），库的义务收窄为「读面保持 pub 可实现」
  （现状已满足）。进库门槛：出现第二个真实多执行者需求 + 编排面/TState 定型。

### Phase 2: emit 通道 + 投影器 + 改名（首个行为变化点）

- `state.json` → `view.json`（各步 `state.json` → 投影器产物）——消除与
  ctx.state 的字面撞名。
- 本地 `emit(name, value)` 通道（事件形状用 aa-workflow 的 `WorkflowEvent`——
  LocalDub 已依赖 `aa_workflow_core` git 源）+ 投影器（`fold(events) → view` 文件，
  落盘防抖收在投影器内部）。
- 职责边界对齐引擎: **步边界事件（Running / Success / Failed）由执行器发**；
  **步骤只发 progress / custom**（与引擎「STEP 事件在耐久边界由引擎发」同构）。
- `set_step` / `set_workflow` / 直写文件的调用点全部退役（清单见 §5.5，28 处
  set_step + 3 处 out.json）——步骤不再碰任何 state 文件；`view` 文件唯一写者
  = 投影器（并发写问题结构上消失）。
- 崩溃一致性: 投影器是纯函数 `fold(events) → view`，可从事件流重建
  （status 可重建；progress 瞬时，重建后归零）。

### Phase 3: 接入（换源表见 §4）

- parallel.rs / StepDag 退役（`DagShape` + 引擎 dataflow 调度接管）；
- `MyStepCtx` → `StepCtx<Input, TState>` 类型替换（body 零改动）；
- 步骤闭包搬进 DAG 节点闭包（`spawn_blocking` 包装阻塞子进程编排）。

#### 引擎路径接入决定（2026-11-23 拍板，先记录待实施）

- **驱动走 core 直驱**：`run_workflow(RunWorkflowOptions::new(dub_sf_ocr(), store))`，
  复用现有 `FsRunStore`（实现 core `RunStore`）；不引入 `workflow_runtime` 层——
  runtime 要的是 `WorkflowExecutionStore` 契约，LocalDub 当前没有，收益未现。
  README「已知边界」的取向同此。
- **入口定义为异步**：`start_workflow` / `continue_workflow` 从 `fn` 改为 `async fn`
  （引擎 `run_workflow` 返回事件流、`.outcome().await`；`FsRunStore::new` 内部
  `tokio::spawn` 也要求 runtime 上下文）。**不**用 `run_workflow_sync` / 自建
  nested runtime 把 async 包成 sync——那是在边界上撒谎。
  - 调用链同步改异步，涉及：`cmd/workflows/{start,continue_workflow,workflow}.rs`、
    `server/feat/workflows/queue/mod.rs` 的 `execute_entry`（它已在 `spawn_blocking` 里，
    改成 async 后把 blocking 执行点换成 `.await`）、`server/feat/workflows/mod.rs`
    的两个 `spawn_blocking` 调用点、`cli/src/main.rs` 的 `fn main`（用
    `#[tokio::main]` 或一个 `Runtime::block_on` 包住分派；`packages/cli/Cargo.toml`
    目前无 tokio 依赖，需补（server 已有 tokio + 队列 worker 常驻 `run_worker()`，
    `axum_server.rs:92`）。
- **run_id 规则：按文件名自增**（用户 2026-11-23 定）。一个 video_dir 对应多次 run，
  `<video_dir>/workflow-store/<run_id>/` 下每 run 一个目录（`FsRunStore` 的布局）：
  - start（新 run）：扫 `workflow-store/` 下数字目录名取最大 +1（`1` / `2` / …），
    传给 `RunWorkflowOptions::run_id`。目录存在即记录，重启可续；整体清理后从 1 重来
    （此时旧日志也不在，不会冲突）。
  - **continue（resume）留 TODO**（用户 2026-11-23）：不按「取最大 run_id 复用」草率接入
    ——resume 语义比表面复杂，用户明确表示在开始 continue 前还有更多改进要做。
    本次接入只覆盖 start；`continue_workflow` 的 Engine 臂继续报「开发中」。
  - 不需要独立 counter 文件。`WorkflowArgs` 目前无 run_id 字段，不改配置形状——
    run_id 由执行器在 start 入口按上述规则算出。

#### TanStack「恢复」语义实测（2026-11-24 查本地源码，防误读）

把「协作式暂停」与「崩溃恢复」分开看，两者都在上游有测试，但**不是一回事**：

- **协作式 pause / resume（core 层）**：`packages/workflow-core/tests/engine.durability.test.ts:23`
  的 `recovers an interrupted running execution from checkpoints` 名字容易误导——打断点是
  workflow 主体自己写的 `if (interrupt) await ctx.approve(...)`，测试用布尔变量控制；
  `simulateRestart`（`tests/test-utils.ts:46`）是个**空函数**。它验证的是「主动暂停后
  用同 run_id 恢复，已完成的 step 不重跑」，不是进程崩溃。真正的 pause/resume 机制见
  `docs/concepts/replay-and-resume.md`：`ctx.step` 查日志里的 `STEP_FINISHED` /
  `STEP_FAILED`，命中就返回记录结果、不调 fn。
- **崩溃遗留状态的恢复（runtime 层）**：`packages/workflow-runtime/tests/runtime-driver.test.ts`
  - `:985` `recovers stale running executions during sweep`：人工摆出死亡现场——往 store
    写 `status: running` 的 run + 一条 `STEP_FINISHED { stepId: "first", result: 41 }`，
    用 `leaseOwner: "dead-worker"` / `leaseMs: 100` 认领，再 `sweep(now: 101)` 让租约过期。
    新 worker 接管后 `firstExecutions === 0`（已记录的 step 不重跑）、
    `secondExecutions === 1`（未记录的重跑）。
  - `:1070` `starts stale executions that crashed before core state was created`：
    core state 都没建成就死掉的 run，sweep 从头跑。
  - `:897` `stops new effects after a heartbeat failure and leaves the run recoverable`。
  这些也是**手工构造崩溃后的现场**，不是真的杀进程。
- **对 LocalDub 的意义**：本仓走 core 直驱，没有 runtime 层的 lease / sweep / 心跳，
  上面那套自动接管拿不到。能用的只有 core 的重放语义——主动用**同一个 run_id 再驱动
  一次**时，日志里有 `STEP_FINISHED` 的 step 直接返回结果不重跑。这与「continue/resume
  留 TODO」是同一块硬骨头，不是已经具备的能力。
- 应用层的数据（如下载进度）续传与框架无关，上游测试不覆盖，勿混为一谈。

## 4. 接入日换源表

| 过渡期 | 接入日 | 改动量 |
| --- | --- | --- |
| 本地执行器发步边界事件 | 引擎原生 STEP 事件（追加日志） | 投影器事件源切换 |
| 步骤 `emit("progress", …)`（本地函数） | `sc.ctx.emit(...)` | 函数签名；词汇不变 |
| 步骤闭包 = 业务 + emit | 原样搬进 DAG 节点闭包 | 包 `spawn_blocking` |
| 投影器（输入 `WorkflowEvent`） | 不改 | **零** |
| `view.json` / `init.json` / UI | 不改 | **零** |
| `StepDag` + `parallel.rs` | 退役 | `DagShape`（dag_policies）+ 引擎调度 |

### 4.1 core vs runtime 判断依据（2026-11-23 源码核对）

> 这几轮讨论反复纠正，结论写在这里固定；**真源是本地 TanStack checkout**
> （`lib_ls/learn_ls/workflow`），不是 aa-workflow 的注释，也不是 in-memory 实现。

**方法论（最容易错的一条）：能力挂在引擎，不挂在 store 接口。**
`deliverSignal` 是 runtime 引擎的函数（`workflow-runtime/src/runtime-driver.ts:52`），
它内部才调 `config.store.deliverSignal(...)`（:274/:341）。所以
"某方法只出现在 `WorkflowExecutionStore`" **不能**推出"该能力只有 runtime 有"——
store 接口只是**引擎要求实现者提供的持久化操作清单**，不是能力清单。
"用 store 接口差集推断能力边界"是错误推理（本次讨论多次因此绕圈）。

**core 自带 durable 等待闭环**（不是"只能 replay 已完成 step"）：
core engine 有 `exec_pause`（park）+ `find_resume`（取回 payload）+ 信号消费路径，
`wait_for_event_fulfilled_by_deliver_signal` 测试（`workflow_core/src/engine/mod.rs:2776`）
证明 core 自己就能"投递信号→恢复 run"。TanStack `runtime-model.md:31` 也把
`waitForEvent`/`sleep`/`approve` 列为 **core 拥有的原语**。

**"投递信号"不是 runtime 独有能力——core 就有两种写法**（别把 `deliverSignal`
当成 runtime 相对 core 的增量）：
- **一步**：`RunWorkflowOptions.signal_delivery(SignalDelivery)`——drive 开头引擎
  自己定位等待点、去重、落 `SignalResolved`、继续重放（`engine/mod.rs:2831-2835`
  注释 + `signal_delivery_option_resumes_in_one_step` 测试）；
- **两步**：注释里叫 `deliver_signal`——调用方先把信号写进 store 日志，之后某次
  drive 才看到（`run_workflow.rs:327` / `engine/mod.rs:3`）。它不是一个公开的
  `run_workflow` 函数，而是 core 设计里就有的 durable 等待机制。
  runtime 的 `deliverSignal` 只是把这些能力**包一层挂到 execution store 上**，
  能力本身 core 具备。

**排除定时（sweep）后，runtime 相对 core 真正多的只有两样**：
1. **执行所有权 / lease**——多 worker 互斥、崩溃后 stale lease recovery 接管；
   core 是 headless 单进程模型，没有"多个执行者竞争同一 run"的概念
   （`runtime-model.md:150-162`）；
2. **定时驱动**（timer/cron sweep，见下条并发模型）。
在"驱动一个 run 前进"这件事上，runtime 就是 core 的 thin wrapper——不要再把
"投递信号"或"store 接口方法多"当成它的增量。

**runId 约束（精确边界）**：
- `RunId = string`（`workflow-core/src/types/index.ts:350`），无 UUID 约束；
- 引擎缺省生成 `run_${Date.now()}_${random}`（`run-workflow.ts:264,1743`）——
  不是密码学保证，**不要求全局唯一**；
- 唯一约束来自 API 形状：`loadRun(runId) → 单个`（`runtime/types.ts:324`）+
  `CreateRunResult.kind: 'existing'`（:73-75）——**同一 store 内一个 runId 至多一个
  execution**；跨 store 实例可重复（接口无全局协调）。
- 注意：这个 1:1 是 API 隐含约束，不是 in-memory 的 `Map` 强加的——存储结构
  （表/文件/文档）由实现者自由决定，对外呈现 1:1 即可。
- **LocalDub**：每 video_dir 是独立 store 作用域，`run-1`/`run-2` 自增（见
  PHASE2-EMIT-PLAN §5）在该作用域内唯一，跨目录重复合法，无需 UUID。

**两个 store 的关系**：`RunStore`（core）存 state 快照 + append-only 事件日志；
`WorkflowExecutionStore`（runtime）extends `WorkflowRunStoreAdapterStore`（≈ 前者），
额外加执行编排操作：run 生命周期记录、claim/lease/stale sweep、timer、signal/approval
投递、schedule。真实实现（`in-memory-store.ts` / `workflow-store-drizzle-postgres`）显示
两份 RunState 并存（core 视角的 replay 状态 + runtime 视角的执行记录），
不能合并成一个 store——职责/生命周期/并发语义不同（core 刻意 headless：
"not a scheduler/queue"）。

**runtime 并发模型**：单个 `sweep()` 串行——while 循环每次 `claimStaleRuns({limit:1})`
→ `await driveClaimedRun` → 下一个（`runtime-driver.ts:471`）；并行来自**多 worker 各跑
串行 sweep**，靠 lease claim 分工（`runtime-model.md:150-162`）。单个 run 内部的 step
并发属于 core DAG 层，不是 runtime。

**对 LocalDub 的落点**：当前 15 步同步跑完、无 sleep/approve/崩溃接管需求 →
**只用 core + 中间件（dag_policies），runtime 保持观察**。一旦出现"run 暂停后稍后恢复"
的需求，再上 runtime；届时补的是租约/timer 调度这些**引擎能力**及其持久化
（投递本身 core 已具备，不是缺口），
不是"接个队列"。LocalDub queue ≈ 手写 execution store（run 级状态 + 串行驱动），
但没有 paused/wakeAt/lease/timer。

### 已拍板

1. **Input 一比一镜像 `input.jsonc`**——部分解析字段依赖上游产物
   （`resolve_language` / `src_language` **优先**读 `<asr>/out.json`，但三级回退中
   该文件非必经：无 ASR 时回落到 `workflow.source_lang` → `unknown`，详见 §3
   "解析计算拆半"），import 时无法预构建；导入只补身份事实
   （`workflow.video_dir` 回填——现役实现即如此）。
   `TInput` = `crate::input::Input`（serde/specta 现成）。
2. **依赖随参数变，步骤级边收敛为稳定并集**——配置选择型（subtitle_source
   三选一 / output_mode 双分支）+ 磁盘存在性仲裁型（asr 回退链 / split_audio
   回退），回退候选全部指向同一上游子树 → **`DagShape` 用 union graph 可行**；
   文件级仲裁留在装配层，不进图。
3. **步骤签名两参数**（`sc: impl StepCtxView, deps: XxxDeps`）+ 双 trait 消歧
   （CtxView/StepCtxView，`ctx` 限定符标记工作流级出处）+ 装配层注入（见
   §3 Phase 1）；**params 下发否决**——工作流 input 是工作流级事实，不切片
   成步骤参数（经 ctx 限定访问保持出处）。

### 仍待拍板

1. **checkpoint 内容**: 产物清单 JSON（旧 `drive_step` 的 `step_artifacts` 形状）
   或 Null（短路只看 checkpoint 存在性）。
2. **失败语义**: 引擎调度器 fail-fast（`abort_all`）vs LocalDub 裁决
   （一分支失败不取消、全部收尾汇总）——保持裁决需调度器加选项。
3. **store**: FileExecutionStore（aa-workflow 契约）vs LocalDub 自有 FsRunStore。
4. **TState 形状**: 极薄身份快照（WorkflowState）还是 `()`——步骤零读取
   （§5.4 已证实），不影响步骤签名。

## 6. 待重新评估（触发条件 + 届时形态）

- **库侧 StepCtx view trait**：触发 = 出现第二个真实多执行者需求（非过渡性质）
  + 编排面（gate 讨论结论）与 TState 定型。届时形态：**只读视图 trait**
  （input / state / emit / id / attempt / 取消与预算探针），**不含构造器**
  （引擎自造自用），`ctx.step` bound 保持具体类型——trait 只服务函数作者与
  外部执行者，不进引擎调度路径。库的现行义务：`StepCtx` 读面保持 pub
  （已满足，docs example 于接入后补）。
- **过渡期替代（若单测 step 函数成为痛点）**：`#[doc(hidden)]` 测试构造器或
  `test-util` feature——不引入抽象。

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
| sf_ocr_fix | sf_ocr/ocr_fix.rs:18 | video_dir、frames_path(约定)、source_lang(解析：`<asr>/out.json` 优先，缺则 wf.source_lang/unknown)、args(OcrFixArgs) | **subtitle-ocr-post 子进程** + `llm::ocr_llm_fix`（降级保留原文） | sf_ocr_fix/{segment_filter,segment_filter_llm_fix,srt}.json | ×1（:146） |
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
  1. `src_language` / `resolve_language` → **优先**读 **`<asr>/out.json`**（影响
     sf_ocr_fix / translate / split_audio / asr_ocr_fix 的 Input 构造）；该文件
     缺失时回落到 `workflow.source_lang` → `unknown`（`Option` 语义，非 fail-fast；
     纯 OCR / subtitle 路径走回退）；
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

### 5.8 import 生命周期（「导入不算流程中」= run input 生产者）

`import_video(input) -> (WorkflowState, Input)`（`workflow/import/download.rs:34`）：

| 产出 | 内容 | 去向 |
| --- | --- | --- |
| 物理产物 | `video_source.mp4`（下载/复制+转码）、`audio_source.wav`（抽音频） | 盘（两条根事实的实体） |
| 身份事实 | video_id（目录名）、title（ytdlp 元数据）、source/url | `state.json` 诞生 |
| 探测事实 | frame_rate | `state.json` |
| **修改 Input（唯一一处）** | `input.workflow.video_dir = Some(video_dir)` 身份回填 | 回填后 `write_input` 落盘 init.json；**返回的 Input 与盘上同源同构** |
| 元数据文件 | `ytdlp_info.json` | translate preprocess 可选读 |

- **依赖导入的数据**：`video_dir`（15 步全部身份入参溯源至此）、两条根事实的
  物理文件（所有约定路径的实体根）、`ytdlp_info.json`。
- **链路**：`start_workflow` = `import_video` → 拿回填 Input → 按 ExecPath 分发
  （Engine 臂开发中）——**import 是 run input 的生产者，不是图的节点**
  （`StepName` 枚举无 import，与引擎模型一致）。
- **接入映射**：import → 回填 Input → `start_run(input)` 冻结 → DAG 跑；
  `sc.ctx.input` 即回填后的 Input。
