//! asr 阶段输出类型 (镜像 TS `packages/subtitle-asr/types.ts` + `whisper_types.ts`)。

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::asr::AsrPath;

/// 词级时间戳 (镜像 TS `AsrWord`)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrWord {
    pub word: String,
    pub start: u32,
    pub end: u32,
    pub probability: f64,
}

/// 单段转录结果 (镜像 TS `AsrSegment` = SubtitleSegment & { words?, confidence? })。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrSegment {
    /// 文本 (已 trim)
    pub text: String,
    /// 起始, 单位 ms
    pub start_ms: u32,
    /// 结束, 单位 ms
    pub end_ms: u32,
    /// 词级时间戳 (whisper.cpp `-ojf` + wordsOutput 时填充)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<AsrWord>>,
    /// 段置信度统计 (avg/min, 范围 [0,1])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<AsrConfidence>,
}

/// 段置信度 (镜像 TS `AsrSegment.confidence`)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrConfidence {
    /// 平均置信度
    pub avg: f64,
    /// 最小置信度
    pub min: f64,
}

/// asr 完整输出 (镜像 TS `AsrResult`)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrResult {
    pub result: AsrResultBody,
    pub meta: AsrResultMeta,
}

/// asr 输出主体 (text + segments)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrResultBody {
    /// 完整转录文本 (segments 文本用空格拼接)
    pub text: String,
    pub segments: Vec<AsrSegment>,
}

/// asr 输出元信息 (镜像 TS `AsrResultMeta`)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrResultMeta {
    /// 视频总时长, 单位 ms
    pub audio_duration: u32,
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
    #[specta(skip)]
    pub args: Option<serde_json::Value>,
    /// 实际推理用的音频路径
    pub input_audio: String,
    /// 实时率 RTF
    pub rtf: f64,
}

impl AsrResult {
    /// 读取 asr 步骤产出的 `asr/asr.json` 并反序列化。
    ///
    /// 消费方（asr_fix / asr_ocr*）统一走这里，避免每处重复
    /// `read_to_string` + `from_str`；路径约定由调用方经 [`AsrPath`] 提供。
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("读取 {} 失败", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("解析 {} 失败", path.display()))
    }

    /// 从 `video_dir` 解析 `asr/asr.json` 路径（拼法走 [`AsrPath`] 单一来源）后读取。
    pub fn read_from_video_dir(video_dir: impl AsRef<Path>) -> Result<Self> {
        Self::read(AsrPath::new(&video_dir).as_path())
    }
}

// ---------------------------------------------------------------------------
// <asr>/out.json —— 本步的**产物事实**
// ---------------------------------------------------------------------------

/// `<asr>/out.json`：asr 这一步产出的、供下游消费的**事实**。
///
/// 与 `asr/asr.json`（[`AsrResult`]）分工明确：后者是逐段转录内容（读它要吃下
/// 全部 segments），前者只有一条——**实测语言**。下游（`TargetLang` / `mix_video`
/// 的字体与`srt` 语言选择）只想要这一条，不必扛整个转录结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AsrOutput {
    /// whisper 实测的源语言（如 `"en"` / `"zh"`）。
    ///
    /// 这是 asr 唯一「算不出来、只能实测」的产物，其它语言信息来源都是配置或推断。
    pub language: String,
}

impl AsrOutput {
    /// 写入 `<asr>/out.json`（asr 成功识别后一次性写入，唯一写者）。
    pub fn write(video_dir: &str, language: &str) -> Result<Self> {
        let out = Self {
            language: language.to_string(),
        };
        crate::state::write_step_out(video_dir, "asr", &out)
            .map_err(anyhow::Error::msg)
            .context("写 <asr>/out.json 失败")?;
        Ok(out)
    }

    /// 读 `<asr>/out.json`；文件缺失 → `None`（纯 OCR / subtitle 路径不跑 asr）。
    pub fn read(video_dir: &str) -> Result<Option<Self>> {
        crate::state::read_step_out(video_dir, "asr").map_err(anyhow::Error::msg)
    }
}

/// 「视频目录 → 实测源语言」的便利入口（`asr_language()` 直接吃 video_dir）。
///
/// 解析失败一律当「没跑过 asr」返回 `None`：语言只影响字体/术语等次要决策，
/// 让它缺位（走 `sourceLang` 配置兜底）比让整条 pipeline 崩掉合理。
pub fn asr_language(video_dir: &str) -> Option<crate::r#const::lang::LangCode> {
    AsrOutput::read(video_dir)
        .ok()
        .flatten()
        .map(|o| crate::r#const::lang::LangCode::from(o.language))
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
    pub from: u32,
    #[serde(default)]
    pub to: u32,
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
