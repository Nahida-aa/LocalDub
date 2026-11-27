# 0004. 翻译结构化输出：自实现 tool loop + 可选输入归一化钩子

> 状态：open（部分已落地，分批策略/提示词等未决定）
> 日期：2026-11-27
> 关联：`packages/llm/src/lib.rs`（`chat_with_tool_loop` / `ToolHandler` / `ToolInputNormalizer`）、
> `packages/llm/src/translation_normalize.rs`、`packages/llm/src/arguments_parse.rs`、
> `packages/core/src/steps/translate/batch.rs`
> 触发：102 任务 translate 步骤反复失败——模型不按约定格式输出译文

## 背景：让模型输出指定结构，协议到底保证什么

最初 102 任务的 translate 失败（`{"dst":[...]}` 缺 `}`），一路排查后发现问题不在"提示词写得好不好"，
而在**协议层面对 tool call 的 `arguments` 内部结构几乎没有约束**。

OpenAI 兼容协议规定：
- `tool_calls[].function.arguments` 是一个 **string**（硬规定），内容是"函数参数，JSON 格式"
- 官方明确警告：*"the model does not always generate valid JSON, and may hallucinate parameters
  not defined by your function schema. Validate the arguments in your code before calling your function."*
- **provider 不校验** arguments 是否符合 `parameters` schema——schema 只是发给模型的"建议"

结论：`arguments` 里装什么形状，全靠模型自觉。这是协议留下的空白，必须由客户端补。

## 探索过程：走过的弯路（为避免重蹈）

| 阶段 | 做法 | 为什么走偏 |
|---|---|---|
| 1 | 纯提示词约定 `{"dst":[...]}` | 无 schema、无校验，模型写坏 JSON 就失败 |
| 2 | response_format / json_object（TanStack 路） | 假设 provider 支持 native structured output；agnes 不保证 |
| 3 | tool schema `"1".."N"` + 手写 `collect_translations` 猜 key | 用猜的方式替 provider bug 打补丁，掩盖问题而非解决 |
| 4 | rig 的 output tool 模式 | 内置 `validate(schema,&args)` 校验，无法注入自定义归一化；agnes 填不对照样失败 |

关键纠正：曾断言"agnes 不守 schema"，但证据不足——最初的失败全是**纯提示词**（没传过 schema）；
真正传了 schema 后，agnes 确实不按数字 key 填，但会按 feedback 调整**内容格式**。所以问题不是
"provider 坏"，而是"协议允许模型输出任意形状 + 我们的 schema 形态不匹配它的习惯"。

## 实测证据（mitmproxy 抓包 + rig spike）

`packages/tmp/rig-spike`（独立 crate，rig 0.47.1，`provider.chat()` 指向 agnes）：
- mitmdump 抓 5 轮完整 request/response 到 `capture.jsonl`
- 请求体正确：`tools[0].function.parameters` = `{"1".."5"}` + required 全列
- 响应 `arguments` 顶层字段名**每轮都不同**：`argument` / `arguments` / `tool` / `action` / `content` / `param1` …
  译文藏在某个字符串字段里，`content` 内的格式也每轮变（换行 `1. ` / 冒号 `1: ` / JSON / 数组）
- **字段名是打地鼠，不能硬编码列表**

rig 的 `parse_test` 还验证了解包语义：`ToolFunction::parse` 只在"整个 arguments 是 JSON string"时
`from_str` 解一层，不解字段级包裹；callback 收到的就是模型原始对象。

三个参考库的对照：
- **rig**：默认 output tool + typed `Input`，结果 = `submit` arguments 透穿；失败按 retries 重试
- **aa-ai**：typed `AgentTool::Input`，`from_value` 失败 → error result 回传同 loop；BTreeMap 保 prompt cache
- **TanStack AI**：主要靠 `response_format`；降级 `json_object` + schema 注入 prompt + engine validate

## 已落地的设计（accepted 部分）

### 自实现 tool loop（`packages/llm`）
`chat_with_tool_loop(system, user, handler, opts, max_turns)`：
```
模型调 tool → ① arguments 容错解析 → ② 可选归一化钩子 → ③ 执行
  → 成功：终止并透穿结果
  → 失败：feedback 作为 role:"tool" 回传 → 下一轮模型自我修正
  → max_turns 兜底
```

### 钩子位置与契约（用户确认）
`ToolInputNormalizer` 勾在 **arguments `from_str → Value` 之后、工具 Input 反序列化之前**：
```
arguments (&str) → from_str → Value → [ToolInputNormalizer] → Value → tool Input → typed
```
- **可选**：`ToolHandler::input_normalizer()` 默认 `None`，这是扩展系统的一种实现形式
- 每个工具可提供不同实现（翻译用 `TranslationInputNormalizer`，其他工具按需）
- 钩子只做"形状归一"，输入必是合法 JSON（由 ① 层保证）

### ① 层容错解析（`arguments_parse.rs`，参考 rig）
保证进入钩子的一定是合法 JSON：`parse_arguments_json` 三级尝试——
严格 `from_str` → `repair_json`（转义裸控制字符/非法反斜杠）→ `parse_partial_object`（截断补 `}`/`]`/引号）。
协议明确警告过的"非法 JSON / 截断"在入口救回，不占钩子职责。

### 归一化规则（`translation_normalize.rs`，用户逐条确认）
目标格式恒为 `{"1":"译1",...}`：
- **不硬编码字段名**：递归遍历 arguments 收集所有候选字符串，JSON 容器优先，逐个试，第一个成功即译文
- **对象**（数字/尾数字 key）：按 key 序号，缺号精确报 `缺少序号 N`
- **文本行三种情况**：①全有序号→按自带序号（缺号/重复/超N 报 REJ）②全无序号→按位置、长度必须==n
  ③部分有→自带的用自带、无序号填空洞（`3. text` 保持 3 不记成 2）
- **数组/无序号**：长度必须 == n 才接受（否则无法验证序号映射）
- 冲突一律 REJ，feedback 回传

### 失败诊断持久化
- 归一化失败时 warn 级打**原始 arguments 全文**（2000 字符截断）+ 每个候选的失败原因（debug）
- 每次失败 append 一行到 `<video_dir>/translate/{lang}.normalize-failures.jsonl`
  （`ts_unix` / `expected_n` / `raw_arguments` / `feedback`）
- 理由：模型随机失败不可复现，日志又常被覆盖；留档攒几次才能看出失败模式分布

### 假提供商测试
注入 send 闭包脚本化响应，4 场景全过：归一化生效+成功终止、feedback 回传恢复、max_turns 兜底、
无钩子透传。`cargo test -p llm` 22 个测试全过。

## 未决定（open）

### 1. 分批策略（当前关注）
现状 `BATCH_SIZE = 50`（曾短暂改 25，已改回）。分批的问题是**每批各自跑一次完整 tool loop**
（各自 feedback 重试），增加 LLM 调用开销与延迟，且批间无法共享上下文/缓存/格式经验。

用户方向：**按输入/输出最大 token 限制动态计算批大小**，而非固定值。待定：
- token 怎么估：字符数粗估 vs 引入 tiktoken-rs 精确计数（增加依赖）
- schema 的 N×property 开销：50 句 → 50 个 property + required，是否改成单字段数组 schema
  （固定开销，但之前测过 agnes 对数组输出不稳，归一化器正是为兼容它）
- context limit 从哪来：写死 / provider 配置 / 保守默认
- 是否干脆不分批（整篇一次），仅估算超限时才切

### 2. 提示词与 tool 的重复
`build_translate_system` 规则 5 说"输出 JSON `{"1":...}`"，但 tool loop 要求"调用 submit tool，
arguments 里放译文"——两套指令并存。agnes 自造字段（`argument`/`content`/`action`）可能部分源于此。
待定：是否统一为只讲 tool 调用，或保留 JSON 描述作为 fallback。

### 3. 钩子 API 的后续形态
`TranslationToolHandler` 当前把 `n`（期望句数）绑在 handler 上，每批新建 handler。若跨批复用，
考虑把 `n` 移到 `normalize`/`handle` 参数。当前"每批一个 handler"用法无问题，暂不改。

## 结论

协议把"validate in your code"的责任交给调用方，且 `arguments` 内部结构无约束——这意味着
"客户端归一化 + 自定义钩子和反复试"是唯一可靠路径。已落地的 tool loop + 钩子 + 容错解析 +
失败留档解决了"能不能翻出来"；未决定的分批与提示词问题属于"性能与整洁度"优化，留待后续。
