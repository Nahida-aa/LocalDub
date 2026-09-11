# Workflow / Task Orchestration 调研

> 调研对象:LocalDub `packages/core` 的 TS 核心步骤编排,是否应引入 TS 生态框架替代自研。
> 状态:调研快照 v0.1(2026-09-11)。结论可能在持续学习中修订。
> 关键词:step orchestration / durable execution / DAG / checkpoint / resume

---

## 0. TL;DR(裁决先行)

- LocalDub 核心的本质不是"pipeline",而是**可恢复的「文件产物构建图」(file-artifact DAG)**:
  - `ctx.json` 只是状态索引;真正处于中心位置的是被外部工具(ffmpeg / whisper.cpp / subtitle-finder / subtitle-ocr)直接读写**的产物文件**(`asr.json`、`timings.json`、srt、`tts/wavs/*.wav`)。
  - 阶段的有序数组(`DUB_STAGES` 等)是**隐式 DAG**;`separate` / `asr` / `sf_ocr*` 三路在 `translate` 汇合,是真实存在的 fan-out。
- 调研过的三个候选库(dagflowjs / @octabits-io/flow / TanStack Workflow)分属另外两大家族(declarative DAG 与 durable replay),与「文件产物=状态」模型是**两本账**,都没有 Checkpoint 落盘到文件、被外部工具消费的建模。
- **裁决:不采纳第三方库,自研 ~150 行调度器(方案 C),吸收 octaflow/TanStack 的 gate / retry 分级 / observer 设计。** 概念与语言无关,不阻碍 ld-core Rust 迁移。
- 附:用户对本领域不熟悉 → 第 5 节给了精读路径。

---

## 1. 核心的本质(先定义问题)

### 1.1 状态在哪

| 层         | 内容                                                                                                                | 角色                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `ctx.json` | task 元数据 + 每 stage 的 `status/started_at/completed_at/progress/last_message/error_message`                      | **状态索引**(四态:pending / running / success / failed) |
| 产物文件   | `asr.json`、`translate.[dstLang].json`、`split_audio.json`、`timings.json`、`tts/wavs/NNNN.wav`、分隔后的 vocals 等 | **真正的状态与交付物**,被外部工具读写                   |

关键文件:

- `packages/core/context/context.ts`、`packages/core/context/types.ts` — `TaskStep` 记录与 `setStep/readCtx`
- `packages/core/stages/utils/stages.ts` — `DUB_STAGES` / `DUB_SF_OCR_STAGES` / `DUB_ASR_OCR_STAGES` / `SUBTITLE_STAGES` + `getSteps()` 动态筛选(按 `subtitleSource` 与 `translate.enabled`)

### 1.2 阶段列表 = 隐式 DAG

有序数组是当前实现顺序,但真实数据流是一个 DAG:

```
video_source
 ├── separate ── separate_after       → vocals.wav / mixedVocals.wav / gatedVocals.wav
 │                                    │
 ├── sf_ocr_pre → sf_ocr → sf_ocr_fix →{ocr srt}──────┐
 ├── asr ── asr_fix  →{asr srt}──────────────────────┤  (useSeparated=true 时 asr 反而依赖 separate 产物)
 ├── asr_ocr_pre→asr_ocr→asr_ocr_fix →{asr_ocr srt}──┤
 └────────────── translate ◄───────────────────────────┘   读 subtitleFilePath(三选一)
                        │
                   split_audio ── 还读 vocals 段做 seed 对齐
                        │
                       tts  → tts/wavs/NNNN.wav
                        │
                   mix_audio ── 读 timings.json + tts wavs
                        │
                   mix_video ◄── 原始视频 + mix_audio
```

可配置的 fan-out 与依赖:

- `asr` 读 `vocalsPath`(`useSeparated=true`)或 `video_source`(`useSeparated=false`)→ `packages/core/stages/asr/asr.ts:30-53`。
- **GPU/CPU 由参数控制**:`asr.ts:55-58` 逐 stage 解析 `runtime`/`device`。因此调度器的**资源互斥键必须以「按参数解析后的实际资源占用」为准**,不可硬编码 "OCR=CPU / demucs=GPU"。

### 1.3 现有恢复语义(并行化时的改造目标)

- 线性 resume:`continue` 找到第一个非 success 的 stage 续跑,跳过已成功段 → `packages/core/tasks/continue.ts:79-114`。
- `continueFrom`:把从该 stage 起的 N 个 stage 全重置为 pending,再跑。
- pipeline 切换:补写不存在的新 stage,并**强制重跑 `mix_video`**(不同 pipeline 产物不同)→ `continue.ts:42-70`。
- make 风格 up-to-date 检查已存在:`split_audio.ts:121` 用 `statSync(translationFile).mtimeMs > statSync(vocals段).mtimeMs` 判断是否需要重跑。
- `current_stage` 是单值;并行化后需要 `current_stages: string[]`,影响 App/SSE 的 wire format。

---

## 2. 三大家族的分类框架(task-orchestration 领域的正确坐标系)

| 家族                                | 代表                                                 | 状态模型                                      | 恢复语义                                        | 作者约束                                                 |
| ----------------------------------- | ---------------------------------------------------- | --------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------- |
| ① 声明式 DAG (declarative DAG)      | octaflow、dagflowjs、Airflow                         | 图是值,每步 transition 落库                   | 从失败步 + 其下游后代重跑                       | 把步骤声明成 node,边显式                                 |
| ② 命令式 durable function (replay)  | **TanStack Workflow**、Temporal、DBOS、Inngest       | **append-only event log**;状态由重放派生      | **replay**:handler 从头重跑,log 短路已完成 step | determinism:副作用进 `ctx.step`,用 `ctx.now/uuid`,无随机 |
| ③ 文件产物构建图 (file build-graph) | **LocalDub 核心**、make/just、Dagster asset、Prefect | **文件就是状态**;存在性 / mtime 判 up-to-date | 文件在=完成;continue 走成功段,失败段+后代重跑   | 无(与外部工具天然契合)                                   |

① 与 ② 都属于 durable execution 大族,卖点是"进程死了、机器没了,还能恢复";③ 的恢复完全在文件系统上,天然适配本地 CLI、无常驻服务。

---

## 3. 逐一 dossié

### 3.1 dagflowjs(`abdullah2993/dagflowjs`)

- 定位:轻量、type-safe 的内存 DAG 执行引擎。**0 star / 0 fork / 4 commits** 的玩具级项目。
- 设计:`DagEngine<T>` 单 context 对象,`addNode({ id, dependsOn, shouldRun, validate, cleanup, execute })`,每 node 返回 `Patch` 合入 context;内置 retry(指数退避)/ timeout / onError 策略(`fail|skip|skip-dependents`)/ metrics。
- 与本质的差距:**无任何持久化与 resume**。`execute(initial)` 一次性内存重构 context;进程死了 state 就没了。它解决的是"内存里的一次性依赖图调度",而这正是你改动里**最不需要换**的部分(串行 for 换成 in-degree 队列 ~40 行)。
- 结论:**弃**。

### 3.2 @octabits-io/flow(已更名 `octaflow`)

- 定位:declarative zod-typed DAG 引擎;**"runs on the Postgres you already have"**。pre-1.0(0.13.0),weekly downloads ≈ 4,0 dependents。
- 设计:`buildWorkflow({ type, inputSchema, steps })` 从 steps 的 `dependencies` 推导 DAG → 依赖就绪即执行(auto-parallel + fan-in)→ 每步 transition 持久化 → 崩溃恢复;per-step retry/timeout;**`StepGate` 并发/限流**(可自定义 store/gate/observer 接口);start 幂等、signal/waitForEvent、map、sub-workflow、saga 补偿。
- 与本质的贴合/差距:
  - ✅ 形状最接近:**"依赖就绪即并行 + translate 汇合"** 分毫不差对应你的 DAG。
  - ❌ durability 只由 **Postgres store** 提供;in-memory store 不落盘。你的核心没有、也不该有常驻 DB。
  - ❌ 状态是 DB 行,不是文件;产物由 handler 内部自写,引擎不知道文件的存在——你的 mtime/artifact-exists 模型无法被表达。
- 中间解:它留了 `WorkflowStore` 接口,可自写一个 **ctx.json 文件后端**——但等于给 pre-1.0 库当白嫖并行调度器的代价是电平加深 TS 依赖、锁 API。
- 结论:**不采纳**;按其设计吸收(尤其 `StepGate` 与 isRetryableError 分级)。

### 3.3 TanStack Workflow(`/home/aa/repos/lib_ls/learn_ls/workflow`,学习克隆)

- 定位:**headless** durable execution 引擎,2026 年从 `@tanstack/ai-orchestration` 抽出,pre-1.0。TanStack 生态(与你 AGENTS.md 的 `@tanstack/intent` 同源)。
- 设计:② 命令式闭包——`createWorkflow({...}).handler(async (ctx) => ...)`;副作用全进 `ctx.step(id, fn)`,写 **append-only event log**;resume = 从 handler 顶部重放,log 命中 `STEP_FINISHED` 就短路。持久化边界是可换的 `RunStore`(in-memory / drizzle-postgres / Cloudflare D1)。含 `ctx.sleep/waitForEvent/approve/now/uuid`、版本路由(`previousVersions`)、attach 订阅。
- 与本质的贴合/差距:
  - ✅ headless、无控制平面、store 可换——哲学与你"零常驻服务"一致。
  - ❌ **determinism 契约**对"shell 出去写产物"的步骤是纯负担(每步要包 `ctx.step("id", fn)`,不得用 `Date.now/Math.random`,无环境态)。
  - ❌ log 只记录"这步跑过且返回了 X",和文件是否 up-to-date 是两本账,和你已存在的 mtime 检查冲突。
- 价值:其 `research/` 目录是对本领域最诚实、最适合新手的精读教材(见第 5 节)。
- 结论:**不采纳**;事件日志/replay、版本路由、store 边界的思想可借鉴。

### 3.4 一行注(来自 TanStack `research/` + `docs/comparison.md`)

- **Temporal**:最成熟的 durable workflow 控制平面;重但需自运维集群。
- **DBOS**:Postgres 后端的 durable execution(应用即数据库)。
- **Inngest / Trigger.dev**:托管/自托管的事件任务平台,命令式 step。
- **AWS Step Functions**:托管状态机,YAML/JSON 声明。
- **Vercel Workflow / WDK**:Vercel 托管持久化;`"use workflow"` 指令式。
- **Dagster / Prefect**:数据管线界与 LocalDub 形状最接近的实现,但 Python + 调度器服务;asset=文件 + materialized 状态(概念参考)。
- **Make/just**:文件产物构建图的工业化形态(并行 + up-to-date),但不是库。

---

## 4. 映射与裁决

### 4.1 为什么三个库都不合适(共同根因)

三个库都是 **"frame your code"** 型:要求把步骤关进各自的 ctx / log / node 结构里换持久化。而 LocalDub 的核心是**文件产物 = 状态中枢**,外部工具直接读写文件,ctx.json 只是索引。"此步是否完成"由文件的存在与 mtime 决定,不是由某本 log/某张表决定。硬套任一库都要同时维护两本账,且 dossié 里各自有硬伤(见表)。

### 4.2 候选方案

| 方案  | 内容                                               | 优点                                            | 代价                                            |
| ----- | -------------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| **A** | 采用 octaflow,自写 ctx.json-backed `WorkflowStore` | 白嫖 auto-parallel + gate + observer            | pre-1.0 依赖(4 下载/周)、加深 TS 占比、API 锁定 |
| **B** | 纯自研并行调度器                                   | 零依赖、完全可控、不冲突 Rust 迁移              | 全自己扛                                        |
| **C** | **吸收设计自研**(推荐)                             | 兼得 A 的 gate/retry/observer 设计 + B 的零依赖 | 需要实现纪律                                    |

**推荐 C。** 理由:

1. 你的恢复语义(continue 找首个非 success、continueFrom 后代重置、pipeline 切换 backfill、mtime up-to-date)比任何库都更适合"文件"承载,自研才能无损保留。
2. 并行化的真实工作量不在调度(串行 for → in-degree 队列 ~40 行),而在 **resume 语义改写**(线性 startIdx → 失败节点 + 后代闭包重置)与 `current_stage → current_stages` 的 API 改动——这恰恰是库接管不了、必须自己做的部分。
3. 概念(就绪、互斥、幂等、retry 分级)语言无关,与 ld-core Rust 迁移方向一致;个人经验也建议先动手(done > perfect)。

C 的实现要素(自研调度器候选清单):

- 显式边表:每 stage 声明 `needs: StepName[]`(或按 pipeline 变体的静态边表)
- artifact-exists / mtime 判完成(复用现有逻辑,`split_audio.ts:121` 已有先例)
- in-degree 就绪队列 + 并发上限;**资源互斥键来自「参数解析后的实际 device/runtime」**,同物理资源互斥,其余并发
- continue 改写:失败节点 + 其传递闭包重置为 pending;`continueFrom`/`targetStep` 同理
- retry 分级、gate、observer 事件面借鉴 octaflow/TanStack 的设计(而非代码)

---

## 5. 学习路径(领域新手)

**广度已饱和,建议「精读 + 动手」而非更多调研。**

1. 按顺序精读 `learn_ls/workflow/research/`(TanStack 设计笔记,比营销 README 诚实):
   1. `research/README.md` — 索引与各文档定位
   2. `RESEARCH.md` — 竞争格局落位(Temporal/DBOS/Inngest/Trigger.dev/Hatchet/Cloudflare Workflows/LangGraph)
   3. `WORKFLOW_STORE_RUNTIME_CONTRACT.md` — engine/store 边界,可直接迁移到 ctx.json 关切
   4. `COMPETITOR_GAP_ANALYSIS_2026-05-25.md` — 生产级盲点清单(泄密/定时/版本 drain/队列),当 checklist
   5. `API_CANDIDATES.md` — 三种 API 设计之争,理解"为什么长这样"
   6. 概念文档:`docs/concepts/replay-and-resume.md`(事件日志+重放短路)、`docs/concepts/scheduling.md`、`docs/concepts/primitives.md`
2. 概念 → LocalDub 代码落点映射:

| 概念                              | LocalDub 已有/应落点                     |
| --------------------------------- | ---------------------------------------- |
| retry 分级(isRetryable)           | `@tanstack/pacer` Retryer(tts 段内重试)  |
| gate / 资源互斥                   | 按 runtime/device 解析后的物理资源互斥键 |
| 幂等 / startKey                   | `start` 的 snapshotInput + task 去重     |
| 后代闭包 reset(airflow caught-up) | `continue` 的重写目标                    |
| artifact 物化(Dagster asset)      | `asr.json`/`timings.json` 等产物即状态   |

---

## 6. Rust 生态调研

### 6.1 调研动机

ld-core 已存在(`packages/core/Cargo.toml`),核心 pipeline runner 未来迁移 Rust 时不依赖 TS runtime。
ld-core 现有依赖已经完备:**tokio**(rt/sync/time,已精简)、**tracing**、**serde_json**、**serde**、**anyhow**、**ffmpeg-next**、**pacer-rs**、**config-rs**。自研调度器的基础只需在此之上加 **petgraph**(拓扑排序/DAG)与一个资源信号量(porock/`tokio::sync::Semaphore`)。

### 6.2 landscape 总览

| 候选                              | 状态             | 持久化模型                                                                      | determinism 约束                                                 | 需 DB / Server        | 动态阶段列表兼容 |
| --------------------------------- | ---------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------- | --------------------- | ---------------- |
| **temporalio**(Temporal Rust SDK) | Public Preview   | Temporal event history                                                          | replay 重放                                                      | ❌ 需 Temporal server | ✅               |
| **restate_sdk**                   | Beta             | Restate server                                                                  | replay                                                           | ❌ 需 Restate server  | ✅               |
| **durare**(DBOS Rust)             | v0.3,new         | Postgres / **SQLite**(文件 DB) / InMemory                                       | **control flow 必须 deterministic**(步骤顺序固定)                | ❌ 需 DB              | ❌ dealbreaker   |
| **iopsystems/durable**(56★)       | Active           | Postgres + WASM component                                                       | WASM sandbox                                                     | ❌ Postgres + WASM    | ❌ WASM 限制     |
| **sayiir**(71★,v1.0.0,MIT)        | **最接近的候选** | **`PersistentBackend` trait**:SnapshotStore + SignalStore,仅需实现 **8 个方法** | **无 replay、无 determinism 约束**,continuation-based checkpoint | ✅ 可做 FileBackend   | ✅               |
| **自研**                          | N/A              | 文件即状态                                                                      | 无约束                                                           | ✅ 无依赖             | ✅               |

### 6.3 sayiir — 唯一值得展开的候选库

- **定位**:2026-02 起源,v1.0.0 stable(MIT,71★,~2700 downloads)。Rust core + Python/Node/CF Workers bindings。`sayiir-core` + `sayiir-runtime` + `sayiir-persistence` + `sayiir-macros`。
- **核心卖点**:`No replay, no determinism constraints`——步骤就是 async fn,无 DSL/无 `ctx.step` 包装,continuation-based:checkpoint 是 snapshot,进程重启后从最后 checkpoint 恢复,不重放整个执行历史。
- **`PersistentBackend` trait**(`sayiir-persistence`):`SnapshotStore`(5 方法:save/load/delete/list/get_status) + `SignalStore`(3 required + 3 default 方法),合计仅需实现 **8 个方法**。`InMemoryBackend` 已提供;Postgres 后端 stable。
- **FileBackend 可行性**:把 `WorkflowSnapshot` 序列化写到 `<task_dir>/.sayiir-snapshot.json`(JSON/serde_json,与现有 ctx.json 同风格);`SignalStore` 的 cancel/pause 映射到现有 `TaskStep` 的 `status` 字段。实现量约 100 行 Rust。
- **与 LocalDub 匹配度**:
  - ✅ 步骤 = async fn,ffmpeg/whisper 子进程调用、文件读写、随机数均无须包装 → 与 TS 模型完全一致
  - ✅ 嵌入式,无 server/无 sidecar
  - ✅ fork/join 并行内置(fan-out/fan-in)
  - ✅ OpenTelemetry tracing 内置
  - ❌ **两本账问题仍存**:sayiir checkpoint 记录的是"步骤的返回值",而不是"产物文件是否存在";`split_audio.ts:121` 的 mtime 检查无法被 sayiir 表达
  - ❌ 2700 下载,v1.0.0 是刚发布版本,社区有限
- **结论**:技术上可行,但收益(内置并行 + telemetry)不足以抵消代价(两本账 + 额外抽象层 + 年轻依赖)。**watch candidate,当前不采纳。** 若将来 sayiir 成熟到 2.0+ 或 FileBackend 由社区提供,可重新评估。

### 6.4 durare — DBOS Rust,determinism 是 dealbreaker

- **定位**:DBOS-compatible durable-execution library,2026-06 起,MIT/Apache-2.0。v0.3。Postgres / **SQLite**(文件 DB) / InMemory 三后端;SQLite 号称"full durability on a file database"。
- **设计**:`#[durare::workflow]` + `#[durare::step]`,per-step 在 DB 记录 completion;crash 后从第一个未 checkpoint 的步骤恢复;步骤结果 exactly-once(`operation_outputs` 表 per-execution counter)。
- **为什么不适合 LocalDub**:
  - **dealbreaker**:DBOS 用 deterministic per-execution counter 索引步骤(每步在 workflow 中有固定序号)。LocalDub 的 `getSteps()` 根据 `subtitleSource`、`translate.enabled` 等动态裁剪阶段列表——每次 run 步骤序列可能不同 → counter 不匹配 → 不兼容。
  - 控制流必须 deterministic:步骤必须按相同顺序调用;LocalDub 有 config 动态裁剪。
  - 需要 SQLite/Postgres:虽 SQLite 是文件 DB,但仍引入 DBMS 抽象层;而 LocalDub 的产物文件需要被 ffmpeg/whisper 等外部工具直接读写。
- **结论:不采纳。**

### 6.5 其他 one-liners

- **temporalio**(Rust SDK):Public Preview,需要 Temporal server(前端 + history + matching + worker + DB)。重量级控制平面,不适合零常驻 CLI。
- **restate_sdk**:需 Restate server;工作在 WASM sandbox;不适合本地 CLI。
- **iopsystems/durable**(56★):Workflow 作为 WASM component 运行,Postgres 做状态存储;WASM sandbox 限制 + 需 PG。
- **durable-workflow/sdk-rust**:client/worker SDK for their orchestration server;server-based → 需常驻服务。

### 6.6 裁决

| 方案                        | 内容                                                                    | 优点                                                                                 | 代价                                                                                          |
| --------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| **A(自研,推荐)**            | petgraph + tokio + porock/Semaphore + serde_json checkpoint 文件        | 零新依赖(petgraph 是唯一大依赖);文件即状态,与 TS 版 make 模型完全一致;控制权完全在手 | 全自己扛                                                                                      |
| **B(sayiir + FileBackend)** | 实现 8 方法 `SnapshotStore` + `SignalStore` → `task_dir/.snapshot.json` | fork/join 内置;OpenTelemetry tracing 内置;步骤就是 async fn(无 DSL/无 determinism)   | 两本账(artifact 文件与 sayiir snapshot 并存);2700 下载年轻依赖;split_audio mtime 逻辑无法表达 |

**推荐 A(自研)。** 理由:

1. 文件即状态是 LocalDub 核心本质,自研才能无损保留(`split_audio.ts:121` mtime 逻辑等)。
2. ld-core workspace 已具备 tokio/tracing/serde_json 基础,自研增量依赖极小(仅 petgraph)。
3. 代码量:显式边表 + petgraph toposort + in-degree 就绪队列 + resource-aware gate + continue 闭包重置 ≈ 150-200 行 Rust,与 TS 版方案 C 对等。
4. 与 ld-core 风格一致,不引入额外架构抽象层。

### 6.7 自研实现要素(Rust 版)

- **显式边表**:每 stage 声明 `needs: Vec<StepName>`(或按 pipeline 变体的静态边表),用 petgraph 构建有向图 → `toposort()` 得到拓扑序
- **就绪队列**:petgraph `Graph::neighbors_directed(Incoming)` + in-degree 计数器,完成一步后递减;in-degree=0 入队
- **资源互斥门控**:按参数解析后的 `runtime`/`device` 生成资源占用描述符,同物理资源互斥 → `tokio::sync::Semaphore`(已由 ld-core 引入 tokio)
- **checkpoint 文件**:`<task_dir>/.pipeline-checkpoint.json` → serde_json,记录每 stage 的 `status/started_at/completed_at/error_message`,与现有 ctx.json 同格式
- **continue 语义**:失败节点 + 其传递闭包(petgraph reachable)重置为 pending;`continueFrom`/`targetStep` 同理
- **observer**:`tracing::info!`(ld-core 已有 tracing 依赖 + tracing-subscriber),每个 stage 的开始/完成/失败 emit span
- **从 TS 渐进迁移**:先在 TS 侧验证 DAG 边表 + resume 语义,再用 Rust 实现同规格的调度器并替换 `runPipeline`/`continuePipeline`

---

## 附:参考与来源

- LocalDub 核心代码(TS):`packages/core/stages/`、`packages/core/tasks/start.ts`、`packages/core/tasks/continue.ts`、`packages/core/context/`
- LocalDub 核心代码(Rust):`packages/core/Cargo.toml`(ld-core)、`packages/pacer-rs/`
- TS 候选库:github.com/abdullah2993/dagflowjs;npmjs.com/package/@octabits-io/flow(renamed octaflow)
- Rust 候选库:github.com/sayiir/sayiir(sayiir v1.0.0,PersistentBackend trait);github.com/SamuelXing/durare(durare v0.3,DBOS-compatible)
- Rust 其他:github.com/temporalio/sdk-rust(temporalio Rust SDK);github.com/restatedev/restate(restate_sdk);github.com/iopsystems/durable(durable-execution,WASM+PG)
- 学习克隆:learn_ls/workflow(TanStack Workflow,含 research/ 与 docs/)
- 调研交互结论:2026-09-11,TS 三库(dagflowjs / octaflow / TanStack) + Rust 四库(temporalio / restate / durare / sayiir)逐一评估后给出方案 C(自研)
