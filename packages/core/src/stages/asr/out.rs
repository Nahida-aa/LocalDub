//! asr 阶段输出类型 (镜像 TS `packages/subtitle-asr/types.ts` + `whisper_types.ts`)。

use serde::{Deserialize, Serialize};

/// 词级时间戳 (镜像 TS `AsrWord`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrWord {
    pub word: String,
    pub start: u64,
    pub end: u64,
    pub probability: f64,
}

/// 单段转录结果 (镜像 TS `AsrSegment` = SubtitleSegment & { words?, confidence? })。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrSegment {
    /// 文本 (已 trim)
    pub text: String,
    /// 起始, 单位 ms
    pub start_ms: u64,
    /// 结束, 单位 ms
    pub end_ms: u64,
    /// 词级时间戳 (whisper.cpp `-ojf` + wordsOutput 时填充)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<AsrWord>>,
    /// 段置信度统计 (avg/min, 范围 [0,1])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<AsrConfidence>,
}

/// 段置信度 (镜像 TS `AsrSegment.confidence`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfidence {
    /// 平均置信度
    pub avg: f64,
    /// 最小置信度
    pub min: f64,
}

/// asr 完整输出 (镜像 TS `AsrResult`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResult {
    pub result: AsrResultBody,
    pub meta: AsrResultMeta,
}

/// asr 输出主体 (text + segments)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResultBody {
    /// 完整转录文本 (segments 文本用空格拼接)
    pub text: String,
    pub segments: Vec<AsrSegment>,
}

/// asr 输出元信息 (镜像 TS `AsrResultMeta`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResultMeta {
    /// 视频总时长, 单位 ms
    pub audio_duration: u64,
    /// 运行设备
    pub device: String,
    /// 检测到的语言代码 (如 "en"、"zh")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_language: Option<String>,
    /// 引擎名 ("whisper.cpp")
    pub engine: String,
    /// 模型路径
    pub model: String,
    /// 原始 asr 参数 (序列化回写, 便于审计)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// 实际推理用的音频路径
    pub input_audio: String,
    /// 实时率 RTF
    pub rtf: f64,
}

// ---------------------------------------------------------------------------
// whisper.cpp `-ojf` 原生 JSON 结构 (镜像 TS `WhisperJson` / `WhisperJsonSegment`)
// ---------------------------------------------------------------------------

/// whisper.cpp 输出的单个 token
#[derive(Debug, Clone, Deserialize)]
pub struct WhisperJsonToken {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub offsets: WhisperOffset,
    #[serde(default)]
    pub p: f64,
}

/// whisper.cpp 输出的段 (offsets 单位为 ms)
#[derive(Debug, Clone, Deserialize)]
pub struct WhisperJsonSegment {
    #[serde(default)]
    pub offsets: WhisperOffset,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tokens: Vec<WhisperJsonToken>,
}

/// whisper.cpp 段偏移 (from/to, 单位 ms)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WhisperOffset {
    #[serde(default)]
    pub from: u64,
    #[serde(default)]
    pub to: u64,
}

/// whisper.cpp `-ojf` 顶层 JSON
#[derive(Debug, Clone, Deserialize)]
pub struct WhisperJson {
    #[serde(default)]
    pub result: WhisperJsonResult,
    /// whisper.cpp `-ojf` 的 transcription 为段数组
    #[serde(default)]
    pub transcription: Vec<WhisperJsonSegment>,
}

/// whisper.cpp result 块 (含 detected language)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WhisperJsonResult {
    #[serde(default)]
    pub language: String,
}
