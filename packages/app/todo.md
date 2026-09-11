## debug todo

### 【进行中】Timeline 滚动自动归零 bug（编辑 asr_ocr_fix / 联动删除）

**现象**：滑动区域在编辑/插入轨道后自动滑到开头（滚动归零）。编辑 **asr_ocr_fix** 最稳。

**背景（git 排查）**：

- 好版本 `3d82d87`（Timeline 编辑功能完整）；坏版本 main(`35343f1`)；回归区间 `3d82d87..35343f1` 共 22 提交。
- 已建分支 `fix/3d82d87-正确基线`，基于好版并已提交 2 个干净功能 commit（不含联动删除）：
  - `f09321d` refactor(ui): remove @repo/ui dependency
  - `e5e5d4a` refactor(split_audio): timings 从 translation.json 读
  - 联动删除（曾临时移植可复现 bug）已撤销，未提交。
- 探索：`get_task_ctx` 直接读 ctx.json，写文件不会 invalidate 它 → task_ctx 失效塌缩理论已排除。
- 机制候选（纯静态读不出，需运行时判定）：容器重挂(scrollLeft 天然 0) / 宽度塌缩(totalPx=duration*pxPerMs)clamp 回 0 / scrollTop 垂直复位。
- 关键观察：`TimelineTracks.tsx:63` 滚动容器是外层 overflow-auto（不在 `<For>` 内）；`TaskDetailPage.tsx:130 tracks()` 每次新建对象，任何信号变化都会按新引用重建 `<For>` 轨道组件。

- 【完成】阶段 0（基线确认）：加探针（Timeline.tsx 300ms 采样 + MutationObserver 盯滚动容器直接子节点）。干净分支编辑 asr_ocr_fix **无任何 [TRACE]/[TRACE-mut] 信号**（sl/st/dur/ct/trks/contentW 全稳定，无子节点替换）→ 干净分支无归零。**结论：归零由联动删除移植引入**（非 base/其它功能）。
- 【进行中】阶段 1（联动删除 分层加，每层独立复现→正常才下一层）：
  - A. 【通过】只加 `useViewingTab` 订阅 + 右键「联动删除」空项（onSelect 仅 log，不写不查）→ 编辑 asr_ocr_fix 无归零。
  - B. 【通过】+ `taskCtxQ`（get_task_ctx 并行 observer）→ 编辑 asr_ocr_fix 无归零。**但 B 层也触发 AsrOcrFixTrack onMount（remount）→ remount 不是归零根因**。
  - C. 【命中！】+ `translation_q`（read translation, enabled=tabTracks().includes('translation')）+ `transLang`/`tabTracks`/`transSegments()`/`serializeTranslation()`（只读不写盘；transSegments 渲染读 .length 建响应式）+ STEP_TRACKS/TranslateFile import
    - 现象：编辑 asr_ocr_fix 归零。**关键：无论是否触发 taskCtxQ（get_task_ctx）重新获取/失效都会归零** → refetch 不是触发枢纽，C 层新增代码（translation_q/transLang/transSegments/STEP_TRACKS 订阅）才是根因所在。
    - 已加 TaskDetailPage `[TRACE-ctx]` 打点（status + steps），配合 Timeline `[TRACE]` 探针待抓全因果链。
    - 已回退 C 层全部代码（AsrOcrFixTrack 恢复 B 层：taskCtxQ + viewingTab + 空联动删除按钮 + onMount 打印），提交为基线。
  - B. + `taskCtxQ`（get_task_ctx 并行 observer，与 TaskDetailPage 同 key）
  - C. + `translation_qq` + `transSegments()` + `serializeTranslation()`（只读不写盘）
  - D. + `linkedDelete`/`handleLinkedDelete` 真写盘 + const.ts 把 `asr_ocr_fix` 挂进 `translate` tab
  - 触发归零的层拆到行级再分（如 A 再拆「仅订阅」vs「仅菜单项」）。
- 阶段 2 收尾：修归因行 → 全量验证 → 必要时把 link 用的 transLang/transSegments/step 抽共享 hook → 提交。
- 决策：**先不复用**（复用留在 bug 定位后的收尾重构）。

**相关文件**：

- `packages/app/src/components/pages/task/Timeline/Timeline.tsx`（scrollLeft、useScrollSync、tracksRef/ruler/labelsRef）
- `packages/app/src/components/pages/task/Timeline/tracks/AsrOcrFixTrack.tsx`（本次改动核心=联动删除）
- `packages/app/src/components/pages/task/Timeline/TimelineTracks.tsx:63`（外层滚动容器）
- `packages/app/src/components/pages/task/TaskDetailPage.tsx`（track 组装 / transQuery / taskCtxQ）
- `packages/app/src/components/pages/task/const.ts`（STEP_TRACKS）
- `packages/app/src/hooks/useScrollSync.ts`

## debug todo f290274

1. 加 TrackEditModal + shared.ts 纯函数
2. 加 TranslationTrack 右键菜单
3. 不动 query 层/TaskDetailPage
4.

## tmp-todo

临时的局部todo, 经常修改

### Timeline ruler refactor (aligned with OpenCutClassic)

- [x] Step 1 — `rulerConfig(pxPerMs, fps)` frame-aware 间隔算法
- [x] Step 2 — **两级刻度线**：label 间渲染小刻度 (tickIntervalMs)
- [ ] **刻度视口虚拟化**：全量渲染暂时够用，后续需要再优化
- [x] **标签格式支持帧号**：缩放到帧级时显示 `+FF` 号而非纯 `M:SS`
- [x] **Zoom 锚定到播放头** + 对数映射：缩放时播放头保持在视口内
- [ ] **播放头吸附到帧**：`onSeek` / playhead drag 时对齐到帧边界
- [ ] **磁吸系统**：元素边缘/播放头/书签 snap（后期）

### Timeline: 多轨道自动检测 & 渲染

- [ ] **重构 Track 数据源** — 每个 step 输出文件对应一个 parser 函数，统一输出 `TrackSegment[]`
  - [ ] `parseAsrTrack(file: AsrResult): TrackSegment[]`
  - [ ] `parseTranslationTrack(file: TranslateFile): TrackSegment[]`
  - [ ] `parseOcrTrack(file: OcrResult): TrackSegment[]`
  - [ ] `parseSplitAudioTrack(file: SplitAudioFile): TrackSegment[]`
  - [ ] `parseMergeAudioTrack(file: MergeTimings): TrackSegment[]`
- [ ] **TaskDetailPage 自动检测可用轨道** — 扫描 `taskDir` 下 step 输出文件是否存在，自动填充 `tracks` 数组
- [ ] **轨道显示开关** — 用户可以勾选显示/隐藏某个轨道
- [ ] **轨道顺序** — 按 pipeline 执行顺序排列（asr → ocr → translate → split → merge）
- [ ] **轨道颜色** — 为每种轨道类型预设颜色
- [ ] **轨道高度 & 折叠** — 支持折叠/展开轨道以节省空间
