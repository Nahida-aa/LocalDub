# tts-concurrency — VoxCPM 在线 TTS 并发压测

并发压测脚本：用 sing-box 把代理节点挂到本机不同端口，并行向在线 VoxCPM 站点发 TTS 合成任务，
测量各站点能承受的并发任务数、busy 拒绝阈值，并输出完整结果表格。

## 功能

- **任务定义**：一个任务 = 串行跑完 `split_audio.json` 的 25 段（每段 1 次 `/generate`：
  text=dst 字幕 + 参考音频=对应 vocals wav），多任务之间并行
- **代理挂载**：解析 mihomo 订阅 YAML（vless-reality / hysteria2），每个节点启动一个独立
  sing-box 实例，监听本机独立端口（mixed inbound）
- **连通性预检**：任务下发前先经代理测节点连通性（gstatic generate_204 + 站点 /config 双探针）
- **busy 停止**：站点返回 busy/拒绝（HTTP 409/429/503 或错误文案）即停止该站点后续任务，
  记录第几个请求开始被拒
- **完整合成判定**：`/generate` 成功返回 + 音频完整下载（>1KB）才计为成功段
- **动态适配**：GET `/config` 探测组件签名，自动适配魔乐站（10 参数）与 HF Space（8 参数），
  兼容 Gradio 4/5/6 新旧 API（`/gradio_api/call/{api}` vs `/queue/join`）

## 用法

```bash
cd packages/benchmark/tts-concurrency
bun install   # 首次：安装 yaml / undici

# 完整压测（每站 2 任务 × 25 段）
bun src/run-concurrency-test.ts --max-concurrent 2

# 快速验证链路（每站 1 任务 × 前 2 段）
bun src/run-concurrency-test.ts --max-concurrent 1 --max-segments 2

# 只测连通性（不跑 TTS）
bun src/run-concurrency-test.ts --dry-run

# 自定义站点 / 更多并发
bun src/run-concurrency-test.ts --site "自定义站=https://example.com" --max-concurrent 4
```

### 路径配置

Windows 下中文路径经命令行传参易乱码，优先使用 `paths.json` 配置文件
（默认查找 `packages/tmp/tts-concurrency/paths.json`，其次包目录）：

```json
{
  "nodes": "C:\\path\\to\\subscribe.yaml",
  "split": "g:\\LocalDub\\workfolder\\...\\split_audio\\split_audio.json",
  "vocals": "g:\\LocalDub\\workfolder\\...\\split_audio\\vocals"
}
```

也可用环境变量 `TTS_NODES` / `TTS_SPLIT` / `TTS_VOCALS` 传入；都不传时自动搜索
`g:\LocalDub\workfolder` 下的 split_audio 目录。

### 参数

| 参数 | 默认 | 说明 |
| --- | --- | --- |
| `--nodes <file>` | paths.json | mihomo 订阅文件路径 |
| `--split <json>` | paths.json | split_audio.json 路径 |
| `--vocals <dir>` | paths.json | vocals wav 目录 |
| `--max-concurrent <N>` | 2 | 每站点最多并行任务数（每个任务占 1 个节点） |
| `--max-segments <N>` | 0 | 每任务只跑前 N 段（0=全部） |
| `--request-timeout <S>` | 180 | 单请求超时秒 |
| `--retries <N>` | 1 | 失败重试次数 |
| `--dry-run` | - | 只测连通性 |
| `--site <name=url>` | 魔乐+HF | 自定义站点（可多次） |
| `--max-nodes <N>` | 不限 | 最多启动的 sing-box 实例数 |
| `--work-dir <dir>` | packages/tmp/tts-concurrency | 工作目录 |

### sing-box core

- 优先使用环境变量 `SING_BOX_BIN` 指向的 sing-box.exe
- 其次使用 `packages/tmp/tts-concurrency/sing-box/sing-box.exe`
- 否则自动从 GitHub releases 下载 windows-amd64 版本

## 输出

运行产物在 `packages/tmp/tts-concurrency/`（gitignored）：

```
sing-box/   sing-box.exe + 每节点 config-{port}.json
audio/      每个任务一个子目录，0001.wav ~ 0025.wav
results/    每任务增量 JSON、report.json、results.md（汇总表格）
```

## 清理

脚本结束时自动 kill 所有 sing-box 子进程。若异常退出残留，可手动清理：

```powershell
Get-Process sing-box -ErrorAction SilentlyContinue | Stop-Process -Force
```
