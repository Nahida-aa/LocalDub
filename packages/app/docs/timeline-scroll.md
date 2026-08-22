# Timeline 滚动"归零"bug 调查与基线结论

> 更新时间：2026-08
> 相关分支：`fix/f290274-稳定基线`（干净基线）、`fix/3d82d87-正确基线`
> 相关提交：`3d82d87`（好版本）→ `5b28bd1`（Index 修复）→ `f290274`（回 For）→ `a03a775`（迁移 main，引入回归）→ `0073186`（稳定引用尝试）

## 结论：f290274 是完整干净基线

实测确认 **`f290274`**：
- 编辑 asr_ocr_fix → **不归零**
- 上下文查询（get_task_ctx）刷新 → **不归零**
- 硬刷新（冷启动）→ 轨道**稳定唯一**（TimelineTracks 用 `<For>`，无重复）

即 f290274 满足全部目标（编辑不归零 + 上下文刷新不归零 + 冷启动唯一）。

## bug 机制（已定位）

滚动归零的触发条件，二者缺一不可：
1. 轨道行 **remount（DOM 重建）**，且
2. 重建链路导致滚动容器 scrollWidth/clientWidth 变化 → 浏览器把 scrollLeft clamp 到有效范围（回 0）

历史上两种途径触发：
- **A. 轨道组件内读取 shared query result（`useQuery` 并渲染 `.data`）** 组合 remount → 归零。
  - 仅订阅不读取（B 层）不归零；读取任意 shared query result 都会命中。
- **B. 父层 `tracks()` 组装数组整体重建**（每查一次重建全部 track 对象 → `<For>` 全量重建轨道 DOM）→ 归零。

## a03a775 迁移为何回归

`a03a775` 迁入 main 的 `query.ts`（`use_*_data` hooks）+ `TaskDetailPage` 重构后，**编辑与上下文刷新都归零**，而 Timeline/轨道组件与 f290274 几乎无差。回归必来自 query 层重构。候选触发源：

1. `use_*_data` 每个 hook 内部用 `useParams({ from: "/group/$id/$taskId" })` 构造 taskDir（f290274 用 `props.groupId/taskId`）。
2. `use_translate_data` 内部额外 `use_task_ctx()` —— 在 TaskDetailPage 自己的 `taskCtxQ` 之外**出现第二个 `get_task_ctx` 订阅实例**。

（精确触发点尚未逐帧复现；f290274 基线不归零，故暂时不迁移该层。）

## 决策

- **基线**：`fix/f290274-稳定基线`（f290274 之上）。
- **迁移策略（保守）**：只吸收 main 的**非 query 资产**，保持 f290274 内联 query 结构不动：
  - `TrackEditModal`（通用编辑表单，`extraFields` 插槽）
  - `tracks/shared.ts`：`BaseTrackProps` + `insertAt`/`deleteAt` 纯函数
  - `TranslationTrack` 右键菜单（f290274 无）
  - **不迁** `query_/query_track.ts` 的 hooks 化（回归源）。

## 换用修复（备选，若未来需要 main 的 query 层）

若要迁 query 层而不归零，可给 tracks 稳定引用（`createMemo` + 按 id 缓存轨道对象，`For` 按 item 引用 diff 跳过未变轨道）——提交 `0073186` 曾尝试，但当时未在 a03a775 上验证即转走。此项待后续在干净基线上单独验证后再决定。

---

## 2026-08 追加：归零具有"非确定性 / 双稳态"特征（实验结论需修正）

在 `81cbe86` 纯净基线 + `a03a775` 迁移状态的反复对照中发现：

- **同一份代码，时而不归零、时而"一直归零"**；会话间切换（reload）后才改变。
- 也就是说，问题**不是**由某个确定文件 / 某个确定改动决定，而是**会话级初始化的一个竞争条件**，产生两种稳定态。
- 因此此前把「AsrOcrFixTrack 的 `get_task_ctx` 订阅」「TaskControlPanel main 版」「query hooks 化」分别判为归零源的实验，**均可能被双稳态随机性误导，结论不可靠**。
- "f290274 / 81cbe86 干净"的判定同样**不确定**——可能只是当时未撞上那一种稳定态。

### 最可疑的会话级开关：`duration`/`totalPx` 初始化时序

- `totalPx = props.duration * pxPerMs()`，`duration` 来自 videoViewer store（初始 0），由 `onVideoReady`（video metadata 就绪）设置。
- 若某次 reload 视频就绪晚，`duration` 长时间为 0 → 滚动容器无滚动宽度（`min-width:100%`）→ 任何重排都把 `scrollLeft` clamp 回 0；另一态视频就绪正常 → 不影响。
- 判别方法：归零瞬间看 Console `[TRACE]` 的 `dur:` 是否为 0，以及归零是 `[SET-SL]`（显式）还是 `[RAF/SCROLL]`（scrollWidth 变化）还是 `[CONN]`（容器替换）。

### 架构根治方向（待实现）：把各轨道查询下沉到轨道组件

不再由 `TaskDetailPage` 集中 7 个 query + 组装 `tracks()` 数组下发给 Timeline，改为：

1. 父层只下发**稳定引用**的轨道定义（`id/label/color/filePath/stageName`，不含数据），按 `viewingTab` + `STAGE_TRACKS` + stage status 决定显示哪些行，`createMemo` + 按 id 缓存引用 → Timeline `<For>` item 引用不变 → **轨道行永不 remount**。
2. 各轨道组件（`AsrTrack` 等）内部 `useQuery(read_app_file_text, filePath)` + parse，mutation 后 invalidate 自己的 read key → 仅自身 rerender（行容器 `h-16` 固定，scrollWidth 不变）→ 无 clamp。
3. 只读轨道（merge_audio_timings / split_audio_timings）用通用 `ReadOnlyFileTrack`。

此方案从机制上消除「父层重建 → remount」，比 scrollLeft 恢复更治本；若 `duration` 双稳态另存，还需叠加初始渲染防御（duration 为 0 时不渲染可滚动内容）。

### 实验状态备份

`a03a775` 迁移 + `AsrOcrFix` 订阅的中间实验状态存于 `git stash@{0}`（`wip: a03a迁移+订阅 实验状态`），如需可 `git stash apply` 恢复，非最终成果。

## 2026-08 追加：`query.ts` 集中 hooks 层是稳定复现源（已排除 `enabled` 因素）

使用者实测确认（作为独立判别，区别于上面被双稳态干扰的二分实验）：

- **只要 TaskDetailPage 一接入 `query.ts` 的 `use_*_data` hooks（无论是否传 `enabled?: () => boolean`），归零即稳定复现；不接入（保留内联 query）则不触发。**
- 也就是说 `enabled` 开关不是变量；触发源在集中 hooks 层本身：
  1. 每个 hook 内部 `useParams({ from: "/group/$id/$taskId" })` 构造 taskDir（vs 内联版用 `props.groupId/taskId`）；
  2. `use_translate_data` 内部额外 `use_task_ctx()` —— 在 TaskDetailPage 自己的 `taskCtxQ` 之外出现第二个 `get_task_ctx` 订阅实例。
- 这为「query 层 hooks 化为 a03a775 回归源」提供了**稳定的实证**（此前结论受双稳态干扰而存疑，此条是确定可复现的）。

**决策强化**：`query.ts`（`packages/app/src/components/pages/task/query.ts`）**暂不接入** TaskDetailPage，文件可保留作资产（当前 uncommitted）；"查询下沉到轨道组件"的架构方向不变。