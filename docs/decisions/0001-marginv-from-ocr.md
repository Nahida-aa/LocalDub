# 0001. 用 OCR y_stats 推断字幕 MarginV（ASS PlayResY=288）

> 状态：accepted
> 日期：2026-11-25
> 关联：`packages/core/src/steps/mix_video/mod.rs`（`infer_margin_v` / `probe_style`）
> 实验：[experiments/marginv-playres/](experiments/marginv-playres/)

## 背景与问题

译文字幕烧录的垂直位置由 `mix_video` 的 `MarginV`（force_style）控制。原实现只有
「显式参数 > 默认值（横屏 5 / 竖屏 70）」两档；用户手工调出 `margin_v=35` 才让译文
贴在中文原字幕上方。希望用 OCR 已有的字幕框位置自动推断，省去手调，并保证
「参数 > 推断 > 默认」的优先级（没经过 OCR 时推断为空，回退默认）。

约束：OCR 产物里已有 `y_stats.mode`（字幕带纵向像素值域）和 `video_height`；
目标布局是「译文字幕底边距原字幕**顶部** 4px，不覆盖」。

## 当时的想法（选项）

| 方案 | 依据 | 问题 |
|---|---|---|
| A. 直接用像素差：`MarginV = video_height - mode[0] + 4` | MarginV 当作绝对底边距 | 算出 88，实测最优是 35——ASS 有隐含分辨率缩放，不是 1:1 |
| B. 按 ASS PlayResY 缩放：`(video_height - mode[0] + 4) × 288 / video_height` | ffmpeg `subtitles` 滤镜把 SRT 转 ASS，默认 PlayResY=288，再缩放到真实画面 | 需要实验标定 288 这个常数 |
| C. 不推断，只保留手调参数 | 避免标定 | 每条视频都要手调，不符合自动化目标 |

选 B，但 288 必须用可复现实验标定，不能只凭 libass 默认值假设。

## 验证

### 实验：烧录采样定位 PlayResY

实验：[`experiments/marginv-playres/`](experiments/marginv-playres/)。固定同一帧
（1280×720，中文原字幕 `y_stats.mode=[636,669]`），只对 `force_style` 的 MarginV
取不同值，截图对比译文位置。

| MarginV | 译文可见性 / 位置 |
|---|---|
| 5 | 贴底，与中文重叠 |
| 35 | 中文正上方约 4px（用户实测最优） |
| 120 | 中文上方约 40px |
| 220 | 译文跳到画面顶部 y≈140 |
| 280 | 几乎贴顶，部分被裁 |
| 285/286/287 | 递减，顶部露出行数逐渐减少 |
| 288 | 几乎消失 |
| 289 | 极少 |
| 290 | 完全消失（截图 hash 与 320 相同） |

**关键事实**：画面高 720，MarginV=290 时字幕底边正好抵达画面顶部（y=0）。
若渲染值 = `MarginV × 720 / PlayResY`，则 `290 × 720 / PlayResY = 720`
→ **PlayResY ≈ 290**，接近 ASS 默认值 288（差几个像素来自描边/阴影）。
用户拍板「就当 288 计算」。

### 公式复算（真实产物）

```
video_height = 720, mode = [636, 669]
MarginV = (720 - 636 + 4) × 288 / 720 = 88 × 0.4 = 35.2 ≈ 35
```

与用户实测最优参数 35 一致。单测 `infer_margin_v_from_ocr_fixtures` 断言 35.2。

### 跨高度验证（回答「288 对其他高度也成立吗」）

初版只用 720p 反推 288，曾被质疑「把单一高度的数当普适常数、没有证据」。
补做跨高度实验：把同一视频缩放到 1440p（2560×1440），同样截 5s 帧，烧不同 MarginV：

| MarginV（1440p） | 截图 md5 前缀 | 译文状态 |
|---|---|---|
| 35 | f4372c… | 译文贴中文上方（与 720p 视觉一致） |
| 288 | d6e43f… | 译文完全消失，只剩中文 |
| 290 / 575 / 580 | 809aab…（三者相同） | 译文消失 |

**关键结论**：1440p 的临界 MarginV 仍是 ~288~290，**没有随画面高度翻倍**（若换算
是固定像素，1440p 临界应到 ~580）。这证实：

- ffmpeg 转 SRT→ASS 时写死 `PlayResX: 384, PlayResY: 288`（`probe.ass` 里核实），
  与视频高度无关；libass 把 288 坐标系整体缩放到真实画面（因子 H/288）。
- 因此「字幕顶到画面顶部」的临界 MarginV ≈ 288 对所有高度恒定——H 在
  `MarginV × H/288 = H` 两边抵消。
- 所以 `(H - mode[0] + 4) × 288/H` 的换算是**普适的**，不是 720p 特例。

**4px 间距的语义**（用户拍板）：是**相对画面比例**，不是固定真实像素。
公式把 `(H - mode[0] + 4)` 整体乘 `288/H`，所以 4 在 ASS 坐标系里也按同一比例
缩放，符合该语义。

### 未验证 / 反例

- PlayResY 用 290 算得 35.4，与 288 的 35.2 在有效区（MarginV≈35）差异可忽略；
  但在极值区（接近 290）两者不同，本决策不覆盖极值。
- 跨高度只在 720p / 1440p 两点测了临界（均为 ~290）；中间高度靠 PlayResY 固定性
  推论，未逐一截图。
- `mode[0]` 必须是「字幕顶部 y」；若未来 post 改了 y_stats 语义需重验。

## 决策

### D1：推断公式

```
MarginV = (video_height - y_stats.mode[0] + 4) × 288 / video_height
```

- 数据：`sf_ocr/frames.json` 的 `meta.video_height` + `sf_ocr_fix/frames_box_adjust.json`
  的 `meta.y_stats.mode[0]`。
- PlayResY 常量取 **288**（用户拍板；实测临界 290）。
- 任一步产物缺失 / 字段非法 → `None`（即「没经过 OCR，推断为空」）。

### D2：优先级

`显式参数 > OCR 推断 > 默认值`（横屏 5 / 竖屏 70）。在 `probe_style` 内用
`cfg.margin_v.or(inferred).unwrap_or(default)` 实现，推断逻辑集中在 `infer_margin_v`。

### D3：就地计算

在 `mix_video` 入口读产物、就地算，不新增跨步骤字段。

## 后果

**正面：**

- 无需手调即可让译文贴合原字幕位置；经验参数 35 被公式复现。
- 参数仍可覆盖，自动化与可控性并存。

**代价：**

- 依赖 OCR 两步产物同时存在；缺任一步就回退默认（可能不理想但不报错）。
- 288 是 ASS 默认分辨率的假设，分辨率极大/极小时未实测。

**重新评估的触发条件：**

- 若 libass / ffmpeg 的 subtitles 滤镜默认 PlayResY 改变，或改用显式 `original_size`，
  重跑实验标定。
- 若 y_stats 语义变化（mode 不再表示字幕带值域），重验公式。
- 若用户要求译文覆盖原字幕而非在其上方，改用 `mode[1]`（底部）重推。
