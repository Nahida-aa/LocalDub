# LocalDub

视频配音 / 字幕流水线：`import → separate → asr / sf_ocr → translate → tts → mix_video`。
本地工具链（ffmpeg / whisper / demucs / 关键帧 OCR）+ 云端服务（LLM 翻译、VoxCPM TTS）。

## 组成

| 路径 | 内容 |
| --- | --- |
| `packages/core`（`ld-core`） | Rust 侧核心：步骤实现、工作流引擎、env 检查、配置模型 |
| `packages/cli` | CLI 入口（Rust），`run-task` 派发各 command |
| `packages/app` + `packages/ui-solid` | Tauri 桌面端。**UI 仍在改造中，暂不保证可用** |
| `packages/server` / `bridge` / `sdk` | RPC 服务、绑定生成、TypeScript SDK |
| `justfile` | 开发命令集合（命令即文档，不再另写用法） |

分层细节见 [docs/architecture.md](docs/architecture.md)。

## 执行路径

`workflow` 的 start 按 `ExecPath` 三选一（`engine` 与 `parallel` 同设报错）：

| 路径 | 开关 | 运行真源 | 状态 |
| --- | --- | --- | --- |
| `Engine` | `--engine` / `workflow.engine` | `workflow-store/<run_id>/logs.jsonl`（事件日志） | start 已接通；continue / resume 未接 |
| `Parallel` | `--parallel` / `workflow.parallel` | 各 `<step>/state.json` | 实验性 |
| `Serial` | 默认 | `view.json` | 稳定 |

`view.json`（任务根）与各 `<step>/state.json` 是给人 / UI 看的**投影**，可丢弃可重建；
引擎的 resume / continue_from / target_step 只认
`<video_dir>/workflow-store/<run_id>/logs.jsonl`。两者不要混为真源，详见 `AGENTS.md`。

## 工作流引擎

start 已由 `ld-core` 直驱 aa-workflow core（`run_workflow` + `FsRunStore`）跑通；
continue / resume 仍留 TODO。采用理由、接入计划、状态快照与踩坑见
[docs/workflow/HANDOVER.md](docs/workflow/HANDOVER.md)。

## 进一步阅读

- `AGENTS.md` — 开发规约、目录约定、已知限制
- [docs/workflow/](docs/workflow/) — 引擎接入计划 / 交接 / 调研
- `input.example.jsonc`、`input.schema.json` — 输入配置面
- `docs/gpu-status.md`、`docs/webgpu-oom.md` — GPU 兼容性与已知故障
