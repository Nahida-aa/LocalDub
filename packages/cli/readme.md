# cli

CLI 端 + core。通过 `input.json` 配置即可运行完整流水线。

## 用法

```bash
cd packages/cli

# 编辑 input.json → 设置 command + 参数 + engines

# 运行
bun run run
```

## input 示例

```json
{
  "$schema": "./input.schema.json",
  "command": "createTask",
  "createTask": {
    "sourceFile": "https://github.com/user-attachments/assets/bd02936f-cf3c-4e4b-85b5-0410d38f69f5"
  },
  "engines": {
    "separate": {
      "runtime": "pytorch",
      "device": "cuda"
    },
    "asr": {
      "runtime": "pytorch",
      "device": "cuda"
    },
    "tts": {
      "runtime": "pytorch",
      "device": "cuda"
    }
  },
  "stages": {
    "translate": {
      "targetLang": "vi"
    },
    "mix_video": {
      "fontSize": 19,
      "marginV": 34,
    }
  }
}
```

## 引擎说明

| stage | runtime | 说明 |
|---|---|---|
| separate | `ort` / `pytorch` | ort=onnxruntime-node(CPU), pytorch=Demucs Python 子进程(cuda/mps/cpu) |
| asr | `faster-whisper` / `pytorch` | faster-whisper=CTranslate2, pytorch=openai-whisper |
| tts | `ort` / `pytorch` / `cloud` | ort=onnxruntime-node,VoxCPM, pytorch=VoxCPM Python 子进程, cloud=远程 API |
| translate | — | OpenAI 兼容 API，从环境变量读取 |

## 阶段参数 (`stages`)

每个 stage 的可选参数，与 `engines` 同层级。当前支持：

| stage | 参数 | 类型 | 说明 |
|---|---|---|---|
| `translate` | `targetLang` | string | 目标语言，如 en, ja, vi；优先于 createTask.targetLang |
| `mix_video` | `fontSize` | number (1-200) | 字幕字号，不填则自动 |
| | `marginV` | int (≥0) | 垂直边距(像素)，不填则自动 |
| | `alignment` | int (1-9) | 字幕对齐: 1=左下 2=中下 3=右下 4=左中 5=居中 6=右中 7=左上 8=中上 9=右上 |
| | `outline` | number (≥0) | 描边宽度(像素)，0=无描边，支持小数如 1.5 |
| | `shadow` | number (≥0) | 阴影深度(像素)，0=无阴影 |

## 其他命令

```jsonc
// 查看任务状态
{ "command": "taskStatus", "taskStatus": { "taskId": "xxx" } }

// 从指定 stage 恢复
{ "command": "resumeTask", "resumeTask": { "taskId": "xxx", "resumeFrom": "tts" } }

// 重跑单个 stage
{ "command": "rerunStage", "rerunStage": { "taskId": "xxx", "stageName": "tts" } }

// 查看设备信息
{ "command": "deviceInfo" }
```
