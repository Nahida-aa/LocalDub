# 实验：MarginV 与 ASS PlayResY 标定

验证问题：`force_style` 的 `MarginV` 与真实画面像素是什么换算关系？能否用 OCR 的
`y_stats` 自动推断出「译文字幕贴在中文原字幕上方 4px」的 MarginV？

## 怎么跑

输入：同一帧（1280×720，中文原字幕 `y_stats.mode=[636,669]`），只改 MarginV。

```bash
cd <repo root>
V="workfolder/师尊带我炸修真/8/video_source.mp4"
S="workfolder/师尊带我炸修真/8/mix_video/vi.srt"
for M in 5 35 120 220 280 285 288 289 290; do
  ffmpeg -v error -i "$V" -ss 5 \
    -vf "subtitles=filename='$S':force_style='FontName=Arial,FontSize=18,PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,BorderStyle=0,Outline=0,Shadow=1,Alignment=2,MarginV=$M'" \
    -frames:v 1 -update 1 -y "mv_$M.png"
done
```

注意：`-ss` 放在 `-i` **之后**（输出 seek）；放前面会导致字幕滤镜时间戳错位，
所有截图相同。

## 输出快照

| MarginV | 截图 md5 前缀 | 译文状态 |
|---|---|---|
| 5 | 65dbce… | 贴底，与中文重叠 |
| 35 | 942c8a… | 中文正上方约 4px（实测最优） |
| 70 | 969a34… | 中文上方约 40px |
| 120 | 2fda29… | 中文上方更远 |
| 220 | f0ea51… | 译文跳到 y≈140（顶部区域） |
| 240 | 214ddc… | y≈100 |
| 280 | 76affe… | 几乎贴顶，部分裁切 |
| 285 | e05d74… | 顶部露约半行 |
| 286 | b4b428… | 递减 |
| 287 | b03d5e… | 递减 |
| 288 | 88a220… | 几乎消失 |
| 289 | 12d9c4… | 极少 |
| 290 | f5f923… | 完全消失（= 320/360/400/700/720 的 hash） |

## 解读

- MarginV 增大 → 译文整体上移；到 290 时底边抵达画面顶部（y=0），完全推出画面。
- 若 `渲染值 = MarginV × video_height / PlayResY`，由 `290 × 720 / PlayResY = 720`
  得 PlayResY ≈ 290，接近 libass/ASS 默认 288。决策取 288。
- 代入真实产物：`(720 - 636 + 4) × 288 / 720 = 35.2 ≈ 35`，与实测最优参数吻合。

## 局限

- 只在 1280×720 一条视频上截图；公式对 video_height 归一化，其他分辨率未逐一实测。
- 临界 290 与假设 288 差 2，来自描边/阴影的额外像素；有效区（≈35）差异可忽略。
- 截图由人眼判定位置（"看不准"的部分由用户提供判断），md5 用于客观区分不同渲染结果。
