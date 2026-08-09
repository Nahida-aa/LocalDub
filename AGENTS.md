<!-- intent-skills:start -->

## Skill Loading

Before editing files for a substantial task:

- Run `bunx @tanstack/intent@latest list` from the workspace root to see available local skills.
- If a listed skill matches the task, run `bunx @tanstack/intent@latest load <package>#<skill>` before changing files.
- Use the loaded `SKILL.md` guidance while making the change.
- Monorepos: when working across packages, run the skill check from the workspace root and prefer the local skill for the package being changed.
- Multiple matches: prefer the most specific local skill for the package or concern you are changing; load additional skills only when the task spans multiple packages or concerns.

<!-- intent-skills:end -->

# LocalDub

- 需要有批判性思维(可以质疑)
- 修改代码后, 如果认为适合提交, 就自行提交
- 测试/实验/探索一律不用 `/tmp`，写到 `tmp/` 或 `packages/tmp/`
- 类型检查: `bun tsc`, `cargo check`
- 可以使用 `gh` 来操作 github, git push 默认由人类进行操作
- 调试日志使用 tracing, 调试之后可以不用清除日志

## Key directories

- `packages/cli/` — 入口 `run-task.ts`（派发器），`src/feat/` 为 pipeline 流程实现
- `packages/core/cmd/` — CLI 命令逻辑（`tasks/task.ts` 集中派发 task 动作，含 `get_group_list`、`taskStatus` 等）
- `packages/cli/src/ml/` — 模型实现（whisper、demucs 等）
- `packages/cli/src/ml/ocr/ocr.ts` — OCR 二进制调用（ort-cpp），使用 `pythonBin()`（config.ts）而非内联 VIRTUAL_ENV
- `packages/subtitle-ocr/` — 字幕专用 OCR 包（ort-cpp、subtitle-node.ts、subtitle-py.py）
- `packages/sf-ocr/` — 关键帧 OCR 策略入口（消费 ocr-lab：subtitle-finder 提关键帧 → subtitle-ocr 识别）
- `packages/benchmark/` — 性能测试与参数对比
- `packages/benchmark/ocr/compute/` — OCR 基准测试脚本
- `packages/benchmark/ocr/compute/postprocess_det.py` — 引用了 `packages/subtitle-ocr/ppocr_keys.json`
- `submodule/whisper.cpp/` — whisper.cpp 官方仓库（GPU Vulkan 构建 → `build/bin/whisper-vulkan`）
- `.agents/hardware.md` — 硬件兼容性 & 已知失败路径
- `.agents/model-strategy.md` — 各模型设备分配详情

## Temp directory

- `packages/tmp/` — 临时文件/构建产物（gitignored via `*/tmp/*`）
- 大型第三方编译（如 ORT 源码）放此目录而非系统 `/tmp/`

## fnrpc

### Macros

- `#[fnrpc::rpc_query]` — query endpoint. Ctx 从 `&T` 参数类型推断，无 `&T` 则 `Ctx = ()`
- `#[fnrpc::rpc_mutation]` — mutation endpoint
- `#[fnrpc::rpc_subscription]` — subscription endpoint
- `fnrpc::fnrpc_registry![fn1, fn2]` — 生成 `build_fn_rpc() -> RpcRouter<Ctx>`（Ctx 从 handler 自动推断）

### Ctx 设计

Ctx 是 **per-request 上下文**，不是全局状态。用户自定义 Ctx 类型，transport 层（Tauri/Axum）每请求构造：

```rust
struct Ctx {
    pub state: AppState,    // 全局共享
    pub headers: HeaderMap, // per-request（Tauri 传空，Axum 传真实值）
}

#[rpc_query]
async fn greet(ctx: &Ctx, input: GreetInput) -> GreetOutput {
    if let Some(auth) = ctx.headers.get("authorization") { ... }
    ctx.state.something()
}
```

| transport | Ctx 构造                                   |
| --------- | ------------------------------------------ |
| Tauri IPC | `Ctx { state, headers: HeaderMap::new() }` |
| Axum HTTP | 从提取器获取 headers/query 填入 Ctx        |

### Router

- `RpcRouter<Ctx>` 内嵌 `Arc<RpcRouterInner>`，clone O(1)
- `.route(handler)` / `.layer(layer)` 消费 `self` 返回 `Self`，链式调用
- `dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr>`
- `path` 是 RPC 方法名（非 HTTP method），与 TS 端 `args.path` 统一

### Middleware

- `TracingLayer`（feature `tracing`）— 每个 RPC 调用 emit `tracing::info!`/`error!`（含 `path`、`input`、`output`、`latency_ms`）
- `HookLayer` — before/after 钩子
- `#[cfg(feature = "tracing")]` gated，需 `tracing_subscriber::fmt::init()`

## Known limits

- **OOM**: torch server RSS > 9.5GB 时可能 OOM。详情 → `packages/research/model-load-benchmarks.md`
- **Dawn WebGPU**：≥3 sessions → `VK_ERROR_DEVICE_LOST`，限制 ≤2 个 WebGPU session
- **ffmpeg swresample whisper 幻觉循环**：sidechain 混音音频在 ffmpeg `-ar 16000` 后尾段产生 x68+ 幻觉循环。已去掉该冗余重采样，让 miniaudio 内部处理。详情 → `.agents/asr-loop-fix.md`
- **whisper.cpp 无法检测短语音**：0.5s+ 的短叹（"唉" 71.20）和轻笑（"哈哈哈" 115.42）在 38 个参数组合中几乎全部 miss。silero VAD v6 能捕获"唉"但 CER 涨 3-4ppt 且时间戳左漂 0.8-1.2s；"啊+哈哈哈"则没有任何参数能捕获——whisper 语言模型解码偏好将短语音合并入相邻段。详情 → `packages/benchmark/asr/whisper/results/FINDINGS.md`
- **VAD 变体时间戳偏移**：所有 VAD 模式都系统性地将分段边界左移（s_off_mean -0.75~-1.85s），导致字幕 timing 不准。CER 最低的 sidechain+vad-v6-th02（8.41%）偏移 -534ms。最佳平衡参数是 sidechain+temp-02（CER 9.48%，s_off +203ms，94.7% 检测率）

## Navigation

- `.agents/hardware.md` — GPU 兼容性 & MES hang 根因
- `.agents/model-strategy.md` — 各模型设备分配策略 & 废弃路径详情
- `.agents/demucs.md` — Demucs CPU fallback 说明
- `.agents/cosyvoice2.md` — CosyVoice2/3 ONNX 导出状态
- `.agents/asr-loop-fix.md` — ffmpeg swresample 导致 whisper 幻觉循环根因
- `.agents/windows-path-case.md` — Windows PATH 大小写坑 (exit=53 + 空输出)
- `docs/webgpu-oom.md` — WebGPU `VK_ERROR_DEVICE_LOST` 根因分析
- `packages/benchmark/asr/whisper/results/FINDINGS.md` — ASR 参数基准测试详细结果（38 组合）
