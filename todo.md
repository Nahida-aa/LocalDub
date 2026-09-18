## Overall Pipeline Status

- ✅ 分离 → ASR → OCR → 翻译 → TTS → 合并，全链路跑通
- ✅ OCR：C++ ORT pipeline + ASR-OCR fusion + 重采样孤立帧
- 🔄 Translate：偶发 Item 16 empty dst，待打印完整 OpenAI 响应分析

## GUI 开发计划（目标：类剪映编辑器）

### Phase 1 — 后端基础设施
- [ ] Axum 加 `/media/{group_id}/{task_id}/*` 路由，serve session 目录文件
- [ ] fnrpc: `get_session_detail` — 聚合 state.json + asr_fix + translation.*.json
- [ ] fnrpc: `update_subtitle_segment` — 写回编辑（timing / 文本）

### Phase 2 — 前端骨架
- [ ] EditorPage 替换 `group/$id/$taskId` stub（左视频 + 右详情 + 下时间轴）
- [ ] 视频播放器 + 播放/暂停/时间显示
- [ ] 播放头和视频同步

### Phase 3 — 时间轴
- [ ] 时间轴容器（缩放 / 平移 / 虚拟滚动）
- [ ] 字幕块按时间段显示
- [ ] 原文轨道 + 译文轨道

### Phase 4 — 波形 + 交互
- [ ] Canvas 音频波形
- [ ] 拖拽字幕块调整 timing
- [ ] 详情面板文本编辑 + 保存

### 已完成的优化项目
- ASR 参数基准测试：sidechain + temp-02 最佳 CER 9.48%，timing 偏移 +203ms
- 2fps select 滤器替换 fps 滤器（解决 edit list PTS 偏移）
- Substring/Triplet 合并消除过渡帧幻觉
- box_y 输出 + 共享合并模块 `ocrMerge.ts`
- C++ ORT pipeline 的 OpenCV 5 兼容修复（geometry module）
- asr_ocr_fix.ts ASR 对齐单帧段 overlap 修复 + 孤立帧重采样
