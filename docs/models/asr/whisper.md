# Whisper 参数参考

使用 runtime：**whisper.cpp** (Vulkan)，模型 `large-v3-turbo`。

## 核心生成参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `--temperature` | `0.0` | 采样温度。越低越确定（重复性强），越高越随机（多样性）。`0.2` 在 CER 和时间戳精度间取得最佳平衡 |
| `--temperature-inc` | `0.2` | 温度递增步长。首次解码用 `temperature`，若 logprob 过低则以 `+inc` 递增重试 |
| `--best-of` | `5` | beam size，保留的最佳候选数 |
| `--beam-size` | `5` | beam search 宽度，越大越准但越慢 |
| `--no-speech-thold` | `0.60` | 无语音阈值。段级 logprob 低于此值判定为静音丢弃。越低越保留弱语音（包括填充词、呼吸声），但可能引入幻听 |
| `--logprob-thold` | `-1.00` | 全局 logprob 阈值，用于拒绝低置信解码 |
| `--entropy-thold` | `2.40` | 熵阈值，用于检测解码失败 |
| `--word-thold` | `0.01` | 词级时间戳概率阈值 |
| `--max-len` | `0` | 段最大字符数。`0`=不限制。配合 `--split-on-word` 可强制短段 |
| `--split-on-word` | off | 按词边界切分而非 token 边界 |

### 实践发现

**Temperature 退火** (`temp-02`)：初始 `temperature=0.2`，温度递增 0.2 直到成功。相比 `temperature=0`（默认），CER 从 8.44% 降到 **7.72%**，时间戳偏移从 +0.09s 降到 **+0.04s**，全部 557 个字符匹配。

**`--no-speech-thold`**：测试了 0.50/0.40/0.20/0.10 四个值，在 raw audio 上 **CER 和段数完全不变**——说明当前场景的"弱语音"不是静音检测的问题。主要瓶颈在语言模型先验，不在 VAD 后处理。

**`--max-len`**：强制短段（20 字符）配合 `--split-on-word` 不影响 CER 也不改善填充词捕获（段数不变）。

## 推理参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `-t, --threads` | `4` | CPU 线程数。当前设为 4，Vulkan 推理主要消耗 GPU，线程数影响解码器部分 |
| `-p, --processors` | `1` | 处理器数（解码器并行） |
| `-ot, --offset-t` | `0` | 时间偏移（毫秒），跳过音频开头 |
| `-on, --offset-n` | `0` | 段序号偏移 |
| `-d, --duration` | `0` | 处理时长（毫秒），`0`=全音频 |

## prompt 控制

| 参数 | 默认 | 说明 |
|------|------|------|
| `--prompt` | `""` | 初始 prompt（最多 `n_text_ctx/2` token）。可引导模型风格/内容 |
| `--carry-initial-prompt` | off | 每段解码前都带上初始 prompt（否则只用于第一段） |
| `--no-fallback` | off | 禁用温度退火 fallback |

### 实践发现

`--prompt + --carry-initial-prompt` 可以在非 VAD 模式下捕获"唉"（sidechain, timing 71.24-71.60 vs GT 71.20，偏移仅 +0.04s），但 **"啊"和"哈哈哈"在 113-116s 区域仍然漏掉**。Vocals 上虽然全部捕获但合并到仅剩 9 段。

结论：`--prompt` 适用于特定场景微调文本风格，无法可靠解决填充词漏标。

## VAD 参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `--vad` | off | 启用 Silero VAD 预分割 |
| `--vad-threshold` | `0.5` | VAD 激活阈值。`0.2` 更敏感但左漂更严重 |
| `-vm, --vad-model` | (内置 v5) | 自定义 VAD 模型路径。支持 v5.1.2 / v6.2.0 |

### 模型版本差异

| 版本 | 文件名 | 特点 |
|------|--------|------|
| v5.1.2 | `ggml-silero-v5.1.2.bin` | 默认，稳定性好 |
| v6.2.0 | `ggml-silero-v6.2.0.bin` | 更灵敏，能检测"唉"和"哈哈哈"，但时间戳左漂 0.5-1.5s |

### 实践发现

VAD 参数对时间戳精度有重大影响：

| 参数 | 数据 | CER | 段数 | s_off | "唉" |
|------|------|-----|------|-------|:----:|
| VAD v6 | sidechain | 10.59% | 88 | -0.79s | ✅ |
| VAD v6 + th02 | sidechain | **7.00%** | 88 | -0.68s | ❌ |
| VAD v5 default | sidechain | 12.93% | 82 | -1.14s | ❌ |

VAD v6 + th02 虽然 CER 最低但时间戳左漂 0.68s，且"唉"被合并入下段。VAD v6 能捕获"唉"和"哈哈哈"但左漂更严重。

**根本原因**：VAD 输出的语音边界包含呼吸/静音前奏，whisper 在此基础上解码时把前序声音包含在段内，导致左漂。

## 抑制参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `--suppress-nst` | off | 抑制非语音 token。方向与 Python API `suppress_tokens=[]` 相反——此参数减少填充词输出 |
| `-mc, --max-context` | `-1` | 最大上下文 token 数。`-1`=不限制 |
| `-ac, --audio-ctx` | `0` | 音频上下文大小。`0`=全部 |

### 实践发现

`--suppress-nst` 在 sidechain 上导致 CER 飙升至 **52.42%**（幻觉激增），raw audio 上无改善。不推荐使用。

## 输出控制

| 参数 | 默认 | 说明 |
|------|------|------|
| `-ojf` | off | JSON full 格式输出（包含 tokens、offsets、概率 `p`） |
| `-l, --language` | `auto` | 语言代码。`zh` 指定中文 |
| `--translate` | off | 翻译到英文 |
| `--diarize` | off | 双声道说话人分离 |
| `--tinydiarize` | off | tdrz 轻量分离 |

## 推荐的 pipeline 参数

当前 pipeline 使用的参数组合：

| 参数 | 值 | 说明 |
|------|----|------|
| 模型 | `large-v3-turbo` | 最佳速度/精度平衡 |
| 设备 | Vulkan (RADV) | 稳定，RTF ~0.09 |
| 音频预处理 | sidechain | 压制 BGM，提升 CER |
| `--temperature` | `0.2` | 最佳时间戳精度 |
| 线程 | `4` | GPU 推理为主，CPU 解码器够用 |

> 完整 38 组参数对比 → `vox-lab/packages/benchmark/asr/whisper/results/FINDINGS.md`
