# merge_audio 设计演进

## Python 原版 (`backend/app/adapters/audio.py` — YouDub-webui 遗留)

```
TTS → audiostretchy (speed = base × localFactor) → 直接拼入 dubbing
```

- **无静音修剪**: TTS 输出带尾部静音一起拉伸
- **双层拉伸**:
  - `baseFactor = totalTarget / totalTTS × 0.99`, 钳位 [0.8, 1.2]
  - `localFactor = desiredSec / (currentSec × baseFactor)`, 钳位 [0.9, 1.1]
  - 最终 `speed = baseFactor × localFactor`, 有效范围 [0.72, 1.32]
- **速度公式**: `speed = desiredSec / currentSec` ❌ 反了
- **变速器**: `audiostretchy` (WSOLA 算法)
- **无中间文件**: 内存拉伸后直接拼入最终 dubbing

## TS 当前 (`packages/cli/src/feat/steps/merge_audio.ts`)

```
TTS → areverse silenceremove → _trimmed.wav
                                    │
                         trimmedSec ≤ slotSec + drift?
                        ╱                      ╲
                     是                       否
                      │                        │
              cp trimmed → stretched    rubberband (max 1.05x) → stretched
                      │                        │
               drift = slot - trimmed     drift = 0 (或 slot - stretched)
                      │                        │
                      └──────── concat ←───────┘
```

### 核心算法：drift 累积

不再逐段独立拉伸匹配 slot，而是让 slack 向后漂移：

```
drift = slotSec - stretchedSec   (正数=提前完成，留给下段)
trimmedSec > slotSec → rubberband (max 1.05x) 追上 slot, drift 清零
trimmedSec ≤ slotSec → 不拉伸, 余量记入 drift, 下段 slot 增大
```

- `slotSec = max(0.05, originalSlotSec + drift)` — floor 防止 negative tempo
- **每段至多 1.05x 变速** — 避免音质劣化；超出的部分继续往后漂
- 只有 trimmed 音频**超过** slot 时才会 rubberband；否则原样保留

### 静音修剪

```
areverse,silenceremove=start_periods=1:start_threshold=-50dB:start_duration=0.05,areverse
```

先反转再 `silenceremove`（识别开头=原尾部静音），再反转回来。这样只砍真正的尾部静音，**不会误伤段内停顿**。

### 调试字段

每段记录 `timings.json` 的 `translation[i]` 中：

| 字段 | 来源 |
|------|------|
| `tts_duration_ms` | TTS 原始 WAV 时长 |
| `stretched_duration_ms` | 修剪+拉伸后的实际时长 |

## 对比

| 方面 | Python 原版 | TS 当前 |
|------|:---:|:---:|
| 速度公式 | ❌ 反 (desired/current) | ✅ drift 累积 + max 1.05x |
| 修尾部静音 | ❌ 无 | ✅ areverse silenceremove |
| 全局时长匹配 | ✅ baseFactor [0.8, 1.2] | ✅ drift 漂移（隐式累计匹配） |
| 变速音质 | ⚠️ WSOLA | ✅ rubberband（仅必要时用） |
| 步骤 | 一步内存操作 | 两步 + 中间文件 |
| 变速范围 | [0.72, 1.32] | max 1.05x（有限变速，余量漂移） |
| 缓存策略 | 按文件存在性跳过 | ❌ 无缓存，每次重建 |
| 输出格式 | 内存 → dubbing | 文件 → stretched/{N}.wav → concat → dubbing |

## 已知问题（两代共有）

- 中间有空白时只插入静音，不做交叉淡化
- BGM 音量固定 0.30，无动态闪避
- drift 只往后传，遇到连续多个超长段时末尾可能累积较大偏移
