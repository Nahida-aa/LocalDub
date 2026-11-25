# 0003. 工作流日志路由：把「写哪个 log 文件」与运行绑定

> 状态：accepted（选 A：span 贯穿全程——从知道 video_dir 起绑定，每处上下文边界开同名 span）
> 日期：2026-11-26
> 关联：`packages/core/src/logging.rs`（`WorkflowFileLayer`）、
> `packages/core/src/integration/workflow/view_span.rs`（tracing middleware）、
> `/home/aa/repos/lib_ls/aa-workflow/packages/workflow_middlewares/src/dag/core/scheduler.rs`
> 触发：引擎路径下 `<video_dir>/<id>.log` 不再生成，修的过程中发现「日志怎么找到文件」是设计问题

## 背景：日志路由的两种思路

引擎路径跑起来后 `<video_dir>/<id>.log` 消失了。排查后发现根因不在某处写错，而在
**「一条日志事件怎么知道该写进哪个文件」这件事本身的设计**。

两种思路：

| 思路 | 谁持有 video_dir | 跨 spawn |
|---|---|---|
| **每个事件自己找文件**（当前实现） | 存在 tracing span 上，layer 收到事件后去查 span 链取 video_dir | 需手动把 span 传播进每个新 task |
| **运行前绑定写入器**（更本质） | 运行开始时就知道 video_dir，把「写这个文件的 logger」与这次运行绑定，之后所有日志自然流入 | 取决于绑定载体能否跨 task |

用户的质疑正是第二种：「运行工作流时不是就已经知道 video_dir 了吗，为什么还要每层去传？」
——这个质疑成立，当前实现走的是生态惯例那条路，不是唯一解。

## 机制约束（已用对照实验验证）

`tracing` 的**当前 span**（`Span::current()`）存在**线程本地变量**里。Tokio 任务不等于新线程，
但会在多线程 runtime 的 worker 线程间迁移；`tokio::spawn` / `JoinSet::spawn` 出来的子任务
**不会继承父任务的 thread-local span**。于是：

```
父任务: current span = workflow{video_dir=58}
   └─ spawn 子任务 → 调度到另一 worker 线程 → current span = 空
        └─ 日志事件 → layer 查不到 video_dir → 不写文件
```

「传播」= 在 spawn 前 `let s = Span::current()` 捕获，在子任务 future 上 `.instrument(s)`，
让子任务每次被 poll 时临时把 span 装到执行线程的 thread-local 上。这是 tracing 官方文档
（`Span::instrument` / `or_current`）推荐的做法。

已实测的两个断链点：

1. `run_workflow` 内部的 `tokio::spawn`（引擎主 task）—— 已由 LocalDub 的
   `view_span_middleware` 在引擎 task 内**重新打开** span 解决（不是在调用侧包，因为子任务看不到）。
2. `dag_to_handler` → `run_dag` 的 `JoinSet::spawn`（每个 DAG 节点一个 task）—— 已在
   aa-workflow 的 `scheduler.rs` 里 `Span::current()` + `.instrument(span)` 解决。

对照实验（假工作流，`cargo test -p ld-core`）：
- 调用侧 `.instrument(span)` 包 `run_workflow(...).collect()`：**不落盘**（证实第一层断链）
- middleware 内开 span + 单节点 handler：**落盘**
- middleware 内开 span + 真实 `dag_policies`：**落盘**
- middleware 内开 span + 真实 `dag_to_handler`（`JoinSet::spawn`）：**修复前不落盘，修复后落盘**

另有一个独立 bug 已修：`%video_dir` 走 `record_debug`，`format!("{value:?}")` 给路径加引号，
导致拼出的 log 路径带引号写不进；middleware 改用裸 `&str`（走 `record_str`），
且 `logging.rs` 的 `record_debug` 增加去引号容错。

## 当前状态（临时方案）

已合入：
- aa-workflow `e386fcd`：`workflow_middlewares` 加 tracing 依赖，`run_dag` 的 spawn 传播 span。
- LocalDub：新增 `view_span` middleware 并注册到 6 个 workflow；保留上述对照测试作为回归测试。

这套能工作，但**每新增一个 spawn 点就要改一次 dag 中间件**——这是 span 方案的固有维护成本。
它把「日志写到哪」的语义分散到了调用链各处的 instrument 调用上。

## 尚未解决的取舍

核心问题：**「运行前就知道 video_dir」这个事实，该用什么载体把绑定关系传下去？**

| 载体 | 跨 spawn | 优点 | 代价 |
|---|---|---|---|
| A. span（当前） | 手动，每处 spawn 一次 | 不改 step 日志；符合 tracing 生态惯例 | 每加 spawn 点就要改调度器；调试隐蔽 |
| B. 事件字段（step 日志自带 `video_dir`） | 天然 | 不依赖 span/thread-local；新 spawn 无需改 | 改几十处 step 日志调用；layer 要改为从事件字段取 |
| C. per-run subscriber/layer（运行前绑定到具体文件） | 仍需在 spawn 处附加 | 最贴合「绑定一次、精确写入」的直觉 | tracing 的 subscriber 也是 thread-local，跨 spawn 同样要传；不比 A 省 |

**关于 publish 回调**：它只是「另一个拿 `WorkflowEvent` 的地方」，而 `drive()` 里的
`while let Some(ev) = stream.next().await` 流消费循环已经逐条拿到同样的事件——
publish 能做的一切都是流消费的子集，且流消费在 driver task 上下文里更好写、更好处理错误。
**不单独列为一个方案**。

**关键洞察**：任何依赖「当前值」（thread-local）的载体——span、subscriber——都逃不过 spawn 断链。
天然跨 task 的只有「跟事件走」（B）。区别只是要不要 step 内部的细节日志。

### 用户的期望（2026-11-26）

> 「关于写入哪些日志，我希望从已知 video_dir 的时候起 就开始写入日志」

指向**运行级绑定**：一旦拿到 video_dir，把「写这个 log 文件」与这次运行绑定，
此后该运行产生的日志都写进去——而不是让每条日志自己去找文件。

当前 span 方案不满足这一点，实证：`drive()` 流式循环里打的
`tracing::info!(target: "workflow_event", ...)`（`engine.rs`）在 driver task 上，
不在 middleware 打开的 span 内 → 只进 stderr、不进 `<id>.log`；
而 step 内部日志在 span 内 → 进文件。同一个任务，两种日志两种去向。

### 待决定

- `<id>.log` 的定位是什么？只要步骤进展/错误（在流消费循环里直接写文件即可），还是要包含 step 内部日志 + driver 侧日志（需要 B 或等价的运行时绑定）？
- 若只写引擎事件到文件，是否回退 aa-workflow 的 tracing 依赖与 scheduler 改动、删掉 LocalDub 的 view_span？
- 若选 B，统一封装入口（宏/函数）以控制改动面，layer 改为「事件字段优先、span 兜底」？
- 能否做到真正的「运行前绑定」：在拿到 video_dir 时构造 per-run 写入器，
  且让它跨 spawn 自动生效（tracing 生态没有现成机制；需自己设计）？
- 新增 DAG 节点 spawn 或新的上下文字段时，是否接受持续修改 `workflow_middlewares`？

## 相关事实

- `WorkflowFileLayer` 当前只从 span 链取 `video_dir`，不从事件字段取（`logging.rs:97-105`）。
- 引擎真源是 `workflow-store/<run_id>/logs.jsonl`；`view.json` 是给 UI 看的投影，
  由 `apply_view_event` 逐条从事件流派生（另见 engine.rs）。
- 日志的 stderr 输出（fmt layer）不受 span 影响，一直可见——这也是为什么问题只表现为
  「终端有日志、文件没有」。

## 结论（2026-11-26）

选 **A：span 贯穿全程**。定位确认为「`<id>.log` = 所有日志，从知道 video_dir 起」，
因此在每个上下文边界开**同名同字段**的 `workflow` span（`video_dir` 用裸 `&str`，走 `record_str`）：

| 上下文边界 | 绑定方式 | 位置 |
|---|---|---|
| import（同步） | `span.enter()` guard 持有到函数结束 | `download.rs` 的 `let video_dir` 之后 |
| CLI driver（async） | `async { ... }.instrument(span).await` | `run_from_imported` |
| UI driver（spawn） | `tokio::spawn(async {...}.instrument(span))` | `server/feat/workflows/mod.rs` 的 `start_workflow` |
| 引擎 task（spawn） | middleware 内 `info_span` + `instrument` | `view_span.rs` |
| DAG 节点（JoinSet::spawn） | `Span::current()` + `.instrument` | aa-workflow `scheduler.rs` |

效果：import 日志、driver 日志（引擎事件）、step 日志都写进同一个 `<id>.log`。
代价：每新增一个 spawn / 上下文边界就开一次同名 span——这是 A 方案的固有维护成本，已接受。
B（事件字段）作为备选保留：若要彻底免维护，可统一改 step 日志自带 video_dir、layer 从事件取。
