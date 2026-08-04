# Timeline 轨道复用设计

> 范围：`packages/app/src/components/pages/task/Timeline/` 及其依赖的 `query.ts` / `query_track.ts`

## 复用策略

- 只抽"行为完全相同"的部分，**不搞配置化抽象**
- 共享纯函数：`tracks/shared.ts` 的 `insertAt`（含 gap/2 + DEFAULT_DURATION_MS）/ `deleteAt` / `linkedDelete` / `LinkedWriteTarget`
- 通用表单：`tracks/comp/TrackEditModal.tsx`（`textLabel`、`initialText/StartMs/EndMs`、`extraFields` 插槽、`onSave`，成功自动 `closeModal`）
- 各轨道组件自包含，差异走各自实现：`AsrTrack` / `AsrOcrFixTrack` / `SplitAudioTrack` / `TtsTrack`（右键菜单样式、defaultRaw 各轨道自留）

## 为什么不用配置化（PR #52 Common* 方案的问题）

- schema 为差异膨胀：`linked` / `isAudio` / `features` / `serialize` 等字段，每个轨道用不同子集
- 重复只是转移：tts 与 timings 轨道的 `serialize` 完全一样（复制粘贴）；数据格式差异配置化消不掉
- 单体组件爆炸：`CommonTrack.tsx` 418 行缝合所有轨道逻辑，改一个影响全部
- 新轨道要先理解 schema 才能写
- **重构导致功能丢失**：为通用化而丢的轨道特有功能（CommonTrack 的编辑弹窗是写死的文本+时间，无 extraFields 插槽）——
  - `跳转到结尾` 菜单：AsrOcrFix/SplitAudio 有，PR 无
  - 置信度/box_y 展示：AsrOcrFix 编辑弹窗有，PR 无
  - 原文/语言展示 + textLabel=译文：SplitAudio 编辑弹窗有，PR 无
  - TTS 状态着色（成功/失败）：TtsTrack 有，PR 无
- 另外 PR 把 `viewingTab` 按阶段过滤改成 `resumeFrom` 分支驱动（设计选择，非功能差异；resumeFrom 轨道分支是 bug 行为），且 `use_track_groups` 比 `use_track` 多 ~249 行

## 代码量对比（功能对等功能等价于：保留全部轨道功能）

- 当前：轨道组件层 705 行 + `use_track` ~89 行 = **~794 行**
- PR #52：CommonTrack/CommonTimeline/CommonTimelineTracks 668 行 + `use_track_groups` 338 行 = **~1006 行**
- 结论：**功能更多、代码更少**（少 ~210 行）

## 数据流

- 数据获取：`../query.ts` 的 `use_*_data` hooks（每阶段一个，内部 `useParams` 构造 taskDir）
- 组装：`../query_track.ts` 的 `use_track`（返回 `() => Track[]`，按 `viewingTab` + `STAGE_TRACKS` 过滤）
- 联动删除：AsrOcrFixTrack 右键"联动删除(校对+译文)"经 `linkedDelete` 同时删 `asr_ocr_fused_llm_fix.json` + `translation.{lang}.json`（`STAGE_TRACKS.translate` 含两条轨道）

## 废弃文件（PR #52 遗留，已删除）

- `CommonTimeline.tsx` / `CommonTimelineTracks.tsx` / `tracks/CommonTrack.tsx` 已删除
- `query_track.ts` 里的 `use_track_groups` 已删除（仅剩 `use_track`）
