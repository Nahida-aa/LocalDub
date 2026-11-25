# 设计决策记录（ADR）

> 约定：模板与状态规则见 `/home/aa/repos/lib_ls/aa-ai/docs/decisions/README.md`（本仓沿用同款格式）。
> 目的：记录「为什么这么干」——完整的需求 → 实验验证 → 结论，而不只是最终结论。

## 目录约定

```
docs/decisions/
  README.md                  # 本文件：索引
  NNNN-kebab-title.md        # 决策，一位一文件，编号递增
  experiments/
    README.md                # 实验索引
    <name>/README.md         # 怎么跑、输出快照、解读、局限
```

## 索引

| 编号 | 决策 | 状态 | 日期 |
|---|---|---|---|
| [0001](0001-marginv-from-ocr.md) | 用 OCR y_stats 推断字幕 MarginV：PlayResY=288，参数 > 推断 > 默认 | accepted | 2026-11-25 |
| [0002](0002-ocr-fix-output-format.md) | OCR 修正输出格式：自定义 `N: 文本` vs JSON（暂保持 text） | proposed | 2026-11-25 |

## 实验索引

| 实验 | 验证的问题 | 结果 |
|---|---|---|
| [marginv-playres](experiments/marginv-playres/) | MarginV 与真实画面像素的换算（ASS PlayResY） | 临界 290 → 取 288；公式复现实测最优 35 |
