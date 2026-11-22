# 交接文档：sf_ocr_dub 接入 aa-workflow 工程

> 交接日期: 2026-11-22。
> 两个仓库: **aa-workflow**（工作流库，`/home/aa/repos/lib_ls/aa-workflow`）+
> **LocalDub**（宿主，`/home/aa/repos/env_ls/LocalDub`，本文所在仓）。
> 主计划: [INTEGRATION-PLAN.md](./INTEGRATION-PLAN.md)——**先读它**（Phase 0-3 /
> 拍板台账 / 15 步接口勘察 §5）。本文是给接手人的状态快照与下一步入口，
> 不重复计划内容。

## 1. 项目一句话

LocalDub 的 `dub_sf_ocr` 流水线（15 步，**盘真值**，自研 pipeline/parallel 执行器）
要接入 aa-workflow 引擎。库本轮已补齐接入所需全部能力；宿主已开始过渡改造
（Phase 0 勘察 ✅ + Phase 1 试点 separate ✅），行为尚未变化。

## 2. 状态快照

### aa-workflow（main，领先 origin 18 提交；工作区干净）

> 状态快照 2026-11-22：HEAD `88d6428`，领先 origin/main 18 个提交（下表 6 个是
> 本轮接入铺路的提交，其余为 docs/改名）。末尾 `88d6428` 是同 LocalDub 配置面
> 对齐的 `videoDir` → `video_dir` 示例/测试改名（纯改名，无逻辑变化）。

本轮 6 个提交（按依赖顺序，为接入铺路）:

| 提交 | 内容 | 为接入铺的什么 |
| --- | --- | --- |
| `505772b` | DubEnv 服务边界 + 故障改环境控制（input 回归纯意图） | 步骤=业务+emit 的形状预演；故障由环境注入（对应 LocalDub 的服务 503 语义） |
| `ccad21f` | dag 变体切声明式调度 + 服务调用全量走环境边界 | dag_policies 在真实流水线上的验证 |
| `19a483d` | 编排统一 DAG 声明（`dag_to_handler` 生成，手写 try_join 删除）；`DagSpec::shape()` | 执行与策略的单一真相源 |
| `4a74217` | `RunCtx<I>` / `StepCtx<I>`——`sc.ctx.input` typed；`DagRunFn = Fn(StepCtx<I>, deps)` | 节点闭包拿到 sc；run 级视图不擦除 input |
| `53bd52e` | `RunCtx<I, S>` / `StepCtx<I, S>`——`sc.ctx.state` 只读快照；`DagSpec<I, S>`；DAG 形态 state=run 常量实测（零 `STATE_DELTA`） | 接入时 TState 极薄也不破坏步骤签名 |
| `fe7cd89` | TState 缺省 Value 的理由 + ctx-state.md 累积三件套 | 文档 |

教义已入档: `docs/concepts/ctx-state.md`（**一句话教义: 对外观测走发送不追加
日志、对内数据走返回值+依赖进 checkpoint，两通道不共用载体**；DAG 形态
state ≡ `initialize(input)` run 内常量；手写形态累积三件套/计数器不进 state）；
AGENTS.md 的 dub 正例表述同步。

### LocalDub（main）

本轮 6 个提交（注意 `59e79706` 是他人无关改动）:

| 提交 | 内容 |
| --- | --- |
| `6cb6a0c4` | **INTEGRATION-PLAN.md** 落档（§0 旧错误解剖 / §3 Phase 0-3 / §4 拍板台账 / §5 15 步勘察） |
| `4276ddd0` | separate 试点 v1（镜像 struct 版 `MyStepCtx`——后被双特征取代） |
| `32cd348b` | 库侧 view trait 评估结论（否决 + 触发条件 + 届时形态） |
| `d3721c74` | Phase 1 细化（三拍板 + import 生命周期 §5.8） |
| `aa9c88aa` | **双特征落地**（`CtxView` / `StepCtxView`）+ separate 迁移到 trait 签名 |

现役执行器**未动**（`pipeline.rs` / `parallel.rs` 照旧跑全流程）；Phase 1 已迁移
**2 / 15 步**（separate、translate）。验证: `cargo test -p ld-core --lib` 全绿（179）。

> 2026-11-22 补记：本节表格只列接入工程本身的提交。其后又完成两轮**配置 wire 名
> 统一 snake_case**（`WorkflowArgs` 与 `steps.*` 全部子结构，含 `gen-input-schema`
> 重生成、example / TS 表单 / server JSON / 测试夹具同步），详见 §5 陷阱 #2。
> 这两轮改动只动配置键名，不改 Phase 1 迁移进度。本仓当前工作区干净、已与
> origin/main 平齐。

## 3. 关键设计决策（已拍板台账——勿重新争论，完整理由见 INTEGRATION-PLAN §4）

1. **Input 一比一镜像 `input.jsonc`**——部分解析字段优先读上游产物
   （`resolve_language` / `src_language` **优先**读 `<asr>/out.json`），import 时
   无法预构建；导入只补身份事实（`workflow.video_dir` 回填——import 是 run input
   的**生产者**，不是图的节点）。注意 src 是**三级回退**（`<asr>/out.json` 实测 >
   `workflow.source_lang` > `unknown`），纯 OCR / subtitle 路径无 ASR 走后两级，
   （2026-11-22 补：Deps 已改为装配层算好的 `src_lang: LangStr` + `target_lang: LangStr`，不再是 Option）——详见 INTEGRATION-PLAN §3。
   **2026-11-22 补**：`Input`（来自 input.jsonc，video_dir 可选）与工作流 Input
   （import 回填后 video_dir 必有）原本共用同一 Rust 类型，类型系统无法表达后者
   的不变量，导致 `video_dir()?` 到处 `Result` + 独立传参。新增 `WorkflowInput`
   （独立结构体，只含 workflow 非 Option + steps；`TryFrom<Input>` 校验 video_dir），
   `video_dir()` 返回 `&str` 不返回 Result——**纯类型保障、零行为变化**。已迁移步骤
   （separate/translate）经 `CtxView::input()` 拿它；未迁移步骤仍吃 `&Input`，逐批切换。
2. **依赖随参数变，`DagShape` 用 union graph**——配置选择型（subtitle_source 三选一
   / output_mode 双分支）+ 磁盘存在性仲裁型（asr 回退链）在步骤级边集合上收敛为
   稳定并集；文件级仲裁留在装配层不进图。
3. **步骤签名 `(sc: impl StepCtxView, deps: XxxDeps)` + 双 trait 消歧**——
   `CtxView`（run 级: input/emit）/ `StepCtxView`（`ctx()` 关联类型访问器 / id /
   attempt）。`ctx` 限定符标记出处: `sc.ctx().input()` 读作「工作流上下文的输入」，
   不是步骤的参数。**工作流 input 不切片、不下发、不重命名**（params 下发否决）。
4. **deps 按消费语义定义，工作流装配层注入**——`TranslateDeps { srt_segs:
   Vec<SubtitleSegment>, src_lang: LangStr, target_lang: LangStr }`：装配层
   `TranslateDeps::new(&WorkflowInput)` 按 `subtitle_source` 选 asr/sf_ocr/asr_ocr
   三来源之一读规范化字幕（缺失/空即就地报错），并用三级回退 + 直通算好本次
   `src_lang`/`target_lang`；步骤体只消费这三份算好的事实，不再读 srt / 现算语言。
   步骤内 dispatch 已反转为装配层注入；存在性仲裁上移装配层。
   （旧 `SubtitleSourcePath` 枚举、`srt_json: NormalizedSrtPath` 设计已废弃。）
5. **入口步骤无 deps**（separate / sf_ocr_pre——只依赖 import 事实）。
6. **state.json ≠ ctx.state**；方向已定: 改名 `view.json` + 步骤去 patch 改 emit +
   投影器折叠事件流（事件形状用 `WorkflowEvent`，LocalDub 已依赖 aa_workflow_core）。
7. **库侧 StepCtx trait 不定义**（构造权/单实现者/面未定三论据；触发条件与届时形态
   见 §6）——消费者端口（六边形）由 LocalDub 自己定义（双特征即端口）。

仍开放 4 项（见 §4）: checkpoint 内容 / 失败语义（fail-fast vs 不取消汇总）/ store
（FileExecutionStore vs FsRunStore）/ TState 形状（WorkflowState 快照还是 `()`）。

## 4. 待办（按优先级）

1. **Phase 1 剩余 13 步批量迁移**（零行为）——**separate + translate 已迁移**
   （translate 一次验证了 `XxxDeps` 注入 + `resolve_language` 拆半（⚠️ src 三级回退、
   非必读 `<asr>/out.json`，Deps 取算好的 `src_lang`/`target_lang: LangStr`——见 §3
   决策 #1）+ 三来源字幕装配三个机制）。继续沿依赖链铺开
   （sf_ocr_pre→sf_ocr→sf_ocr_fix→…）。每步: 签名改 `(sc, deps)`、意图面经
   `sc.ctx().input()` 现取、事实面进 `XxxDeps`、`cargo test -p ld-core --lib` 全绿。
2. **Phase 2**（首个行为变化点）: `state.json → view.json` 改名 + 本地 emit 通道
   （事件形状 `WorkflowEvent`）+ 投影器；**28 个 `set_step` + 3 个 `out.json` 写点
   退役**（逐点 file:line 清单见 INTEGRATION-PLAN §5.5——separate 的 5 处是模板）。
3. **Phase 3 接入**: 换源表 §4；parallel.rs / StepDag 退役（`DagShape` + 引擎调度）；
   `MyStepCtx → StepCtx<Input, TState>` 类型替换。
4. **四个待拍板**（§4）——建议在 Phase 2 结束、Phase 3 开工前逐项定。

## 5. 陷阱与教训（踩过的，别再踩）

1. **`AtomicBool` 派生 `Default` = `false`**——「缺省健康」变「缺省全故障」，库示例
   全红过一次（`ServiceHealth` 已手写 Default 并留 ⚠️ 注释）。凡「默认应为真」的
   布尔包装，Default 一律手写。
2. **wire 名 camelCase 是历史遗留，已改 snake_case**（2026-11-22）：`WorkflowArgs`
   原是 `rename_all = "camelCase"`（`output_mode` / `subtitle_source` 曾靠显式 `rename`
   覆盖成 snake_case），2026-11-22 起统一为 `rename_all = "snake_case"` 并删掉那两个
   显式 rename；`input.schema.json` 由 `gen-input-schema` 重新生成，example / TS 表单 /
   server 侧构造的 workflow 键同步为 snake_case（`video_dir` / `source_lang` /
   `target_lang` / `continue_from` / `target_step` / `queue_id` / `step_name`）。
   同日 `steps.*` 全部子结构体（含 `llm::LlmFixArgs` / `CheckArgs` / `CookieArgs` /
   `EnvArgs`）也统一为 snake_case，example / 测试夹具 / server 构造 JSON 同步。
   **`device-rs` / `time` 里走 fnrpc bindings 的结构体仍是 camelCase**（不在 input
   配置面，未改）。写 fixture / 改 wire 名前先查对应**结构体声明紧邻的**
   `serde(rename_all)`（别拿同文件别处的属性来套）。
3. **闭包 shadow-then-move 陷阱**: `let env = env.clone();` 之后下一个 clone 读的是
   **已移动的绑定**——多闭包消费同一 handle 时，用独立命名从稳定绑定克隆
   （同 `let st = Arc::clone(&state)` 的模式）。
4. **serde 派生 `Default` 的 unit 陷阱**与 1 同源: 派生出的缺省值不一定是你语义上
   的缺省。
5. **文档会过时**: LocalDub `DEPENDENCIES.md`（from_state/from_video_dir 已废）、
   旧 `sf_ocr_dub.rs`/`drive_step`（引擎接入注释中）——**一切以源码为准**；
   INTEGRATION-PLAN §5 的勘察是 2026-11-21 快照，改码后同样可能过时。
6. **aa-workflow 不跑 rustfmt**（基线非 rustfmt 格式）；LocalDub 同样谨慎——
   避免几百行无关 diff。
7. **runtime 至少 2 worker**（若接入用到 block_in_place 桥）——aa-workflow
   AGENTS.md 有完整事故记录。

## 6. 库侧 view trait 评估结论（防重新争论，完整版 §6 of PLAN）

`aa-workflow` **不定义** StepCtx view trait: 构造权（trait 构造器暴露 `EngineRuntime`
pub(crate) 内脏，sealed 又堵死外部实现）/ 单实现者（唯一实现者是引擎自造的
StepCtx）/ 面未定型（编排原语上不上 sc.ctx 的讨论未结、TState 未拍板）三论据。
**触发条件**: 第二个真实多执行者需求 + 编排面与 TState 定型；届时形态: 只读视图
trait（不含构造器），`ctx.step` bound 保持具体类型。消费者端口由消费仓定义
（LocalDub 双特征即先例，接入完成后是 aa-workflow docs example 的真实素材）。

## 7. 关键文件地图

### aa-workflow（库）

| 文件 | 内容 |
| --- | --- |
| `packages/workflow_core/src/define/mod.rs` | `RunCtx<I,S>` / `StepCtx<I,S>` / `BaseCtx::step` / `compensate`（擦除包装） |
| `packages/workflow_core/src/engine/mod.rs` | `exec_step` / `exec_step_unwrapped`（typed input/state 穿线）/ 补偿擦除路径 |
| `packages/workflow_middlewares/src/dag/core/declare.rs` | `DagSpec<I,S>` / `DagRunFn<I,S>` / `define_dag` / `DagSpec::shape()` |
| `packages/workflow_middlewares/src/dag/core/scheduler.rs` | `run_dag` / `ensure` / `dag_to_handler`（泛型 I/S） |
| `packages/workflow_middlewares/src/{dag/policies,reseed}.rs` | dag_policies（DagPlan） / reseed（redoSet） |
| `examples/store_file/src/dub_sf_ocr/mod.rs` | 声明式示例: `DubEnv` + `dub_spec` + 生成 handler（接入的形状预演） |
| `docs/concepts/ctx-state.md` | 教义文档（两通道 / DAG 常量 / 累积三件套） |

### LocalDub（宿主）

| 文件 | 内容 |
| --- | --- |
| `docs/workflow/INTEGRATION-PLAN.md` | **主计划**（Phase 0-3 / 拍板台账 / §5 勘察 15 步 + 写点清单） |
| `packages/core/src/workflows/step_ctx.rs` | `CtxView` / `StepCtxView` / `MyStepCtx`（双特征端口） |
| `packages/core/src/workflows/{pipeline,parallel,dag}.rs` | 现役执行器（run_step 分发 / 并行执行器 / StepDag 声明） |
| `packages/core/src/steps/*/` | 15 步（勘察逐条见 PLAN §5.2；separate 已迁移为样板） |
| `packages/core/src/state/mod.rs` | state 拆分模型（WorkflowState / StepState / out.json） |
| `packages/core/src/integration/workflow/` | 旧引擎接入（**整体注释中**——恢复参考，勿当现行） |

## 8. 验证命令

```bash
# aa-workflow（Postgres 本地可用，先 pg_isready）
cd /home/aa/repos/lib_ls/aa-workflow
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast          # 全绿基线

# LocalDub
cd /home/aa/repos/env_ls/LocalDub
cargo check -p ld-core --all-targets
cargo test -p ld-core --lib                    # 179 全绿（Phase 1 后基线）
```
