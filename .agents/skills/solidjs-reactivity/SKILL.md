---
name: "solidjs-reactivity"
---

## SolidJS 响应式陷阱

### getter 函数 vs 普通变量

**For 的 render 函数中，所有依赖 props/signal 的中间值必须写成 getter 函数，而非普通变量。**

```tsx
// ❌ 错误：ms/isLabel 是普通变量，只在插入时计算一次
<For each={arr}>
  {(i) => {
    const ms = i * props.interval;  // 被闭包捕获的定值
    const isLabel = shouldShowLabel(ms, props.labelInterval);
    return <div style={{ left: `${ms * props.pxPerMs}px` }}>
      {isLabel ? text : null}
    </div>;
  }}
</For>

// ✅ 正确：getter 让 SolidJS 追踪内部依赖
<For each={arr}>
  {(i) => {
    const ms = () => i * props.interval;
    const isLabel = () => shouldShowLabel(ms(), props.labelInterval);
    return <div style={{ left: `${ms() * props.pxPerMs}px` }}>
      {isLabel() ? text : null}
    </div>;
  }}
</For>
```

**原因**：SolidJS 编译 JSX 为 `createEffect`，只追踪在 JSX 内被调用的函数/属性访问。普通变量在闭包外已求值，SolidJS 看不到它们内部依赖的 props/signal。

### 案例：TimelineRuler 刻度不变密

缩放时 `tickIntervalMs` 和 `pxPerMs` 都变化，但已渲染的刻度（小竖线）位置不更新：

- `ms = i * tickIntervalMs` → 普通 number → 不追踪 `tickIntervalMs`
- 只追踪 `pxPerMs` → 位置变了但没变密
- 改为 `ms = () => i * tickIntervalMs` → getter → 追踪 `tickIntervalMs` → 正确更新

### 通用规则

| 写法                                 | 追踪                   | 适用场景                       |
| ------------------------------------ | ---------------------- | ------------------------------ |
| `const x = expr`                     | 不追踪                 | 常量、不依赖 props/signal 的值 |
| `const x = () => expr`               | 追踪 `expr` 内所有依赖 | 依赖 props/signal 的中间计算   |
| `const [x] = createMemo(() => expr)` | 追踪 + 缓存            | 复杂计算，多处引用             |

### TanStack Query 的 `enabled` 不是"刷新触发器"

**`enabled` 只控制查询是否**启动**，不负责已缓存数据的失效/刷新。把它当成"文件存在才读"的条件没问题，但指望 `enabled` 变化去触发重新拉取是错的。**

```ts
// ❌ 误区：以为文件消失/重现、或文件被覆盖重写时，enabled 变化会刷新数据
const asrQuery = useQuery(() =>
  client.read_app_file_text.queryOptions(
    `${taskDir}/asr/asr.json`,
    { enabled: stage_map().asr?.status === "success" }, // 或 file_exists
  ),
);
// 问题：
//  - enabled false→true 只会触发【首次】查询；
//  - enabled true→false 查询停止，但【已缓存数据仍保留】，组件还能读到旧数据；
//  - enabled 始终 true 时，磁盘文件被覆盖重写，enabled 根本不变，缓存【永不刷新】。
```

**正确做法：把"是否该读"和"何时刷新"拆成两件事。**

1. **是否该读（初始条件）**：用 `enabled`（文件存在 / stage 成功均可）。
2. **何时刷新（内容变化）**：靠文件树订阅事件 `watch_task_tree` 收到该路径的 `Changed`/`Created` 后，显式 `queryClient.invalidateQueries({ queryKey })` 让缓存失效并重拉。

```ts
// ✅ 文件树事件 → 失效对应查询（精确匹配路径，可加前端 debounce）
useQuery(() => client.watch_task_tree.streamedOptions(taskDir), {
  onSuccess: (event) => {
    if (event.path.endsWith("asr/asr.json")) {
      qc.invalidateQueries({ queryKey: asrQueryKey });
    }
  },
});

// 二进制（视频/音频）不进 TanStack Query，走 axum ServeDir 静态文件。
// 刷新方式不同：用 Solid 信号给 <video src> 追加 ?v= 版本号强制重拉，
// 而不是 invalidateQueries。
```

**关键区分**：

- **JSON 等小文件**：TanStack Query + `enabled`(初始) + `invalidateQueries`(刷新)。
- **二进制大文件（mp4/m4a/wav）**：axum `/media` `ServeDir` 静态服务（自带 range/ETag），前端用 Solid 信号刷新 `<video src>`（如 `?v=` 版本号），**不**走 TanStack Query。

**为什么这样分**：二进制进 Query 会把大文件塞进内存 cache 且无法利用浏览器 range/seek；ServeDir 直接流式 + range 才是正确的媒体交付方式。两者刷新机制天然不同，别混用。
