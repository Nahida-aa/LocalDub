# 并行步骤的进度条与日志互相践踏（已记录，未改动）

> 日期：2026-11-25
> 状态：known issue，暂不改动

## 事实

LocalDub 的 workflow DAG 由 aa-workflow 的 `dag_to_handler` 按依赖**并发**驱动，不是串行。
`packages/core/src/workflow/workflows/dub_sf_ocr.rs` 顶部 ASCII 图的两条分支
（`separate` 与 `sf_ocr_pre`）并行。

判断并行不能只 grep 本仓 `try_join`：并发在调度器内部（`dag_to_handler` / aa-workflow 引擎），
spec 里没有 `try_join` 字面。

## 现象

并发步骤各有 indicatif 进度条（`\r` 原地刷新）、LocalDub tracing 日志（换行）、子进程
继承 stderr 直接输出，三者共享 stderr 且无协调，终端输出互相穿插。实测日志：

```
INFO separate: spawn .../demucs-wgpu
[00:00:03] 分离 [====>] 14/100%    INFO env: subtitle_finder_bin 注入 LD_LIBRARY_PATH=...
INFO sf_ocr: subtitle-finder ...
[00:00:04] 分离 [======>] 23/100%   [00:00:15] 解码 [===>] 388/3582 帧
INFO separate: [SeparateAfter] sidechain: ...
```

## 判断

- 单个 `pb.suspend()` 包不住——多个步骤各有自己的 ProgressBar 实例并发刷新。
- 根治要所有步骤共享 `indicatif::MultiProgress` 并用 `suspend()` 协调日志，改动面大，跨步骤传递实例。
- TTS 复用段的「上一段 跳过」文案修复（`b9149692`）只解决单步骤内持久 RTF 误导，不属于此并发问题。

## 决定

2026-11-25 用户明确「先记录，但不改动」。除非另行要求，不动进度条/日志基础设施。
讨论终端输出混乱时先认清并发事实，不要当成串行下的进度刷新间隙。
