use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

use crate::context::WorkflowCtx;
use crate::steps::utils::paths::{asr_fix, asr_ocr_fix, sf_ocr_fix};
use crate::workflows::args::SubtitleSource;

/// translate 阶段参数 (镜像 TS `packages/core/steps/05_translate/args.ts` TranslateArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 ctx.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 目标语言统一在 `input.workflow.targetLang` 配置 (任务级概念: 一个任务只有一个翻译
/// 阶段, 不需要 step 级覆盖)。旧的 `steps.translate.targetLang` 已移除,
/// deny_unknown_fields 让残留配置明确报错 (不能静默失效 -> 翻错语言)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranslateArgs {
    /// OpenAI 兼容端点
    #[serde(default = "openai_base_url")]
    pub api_base: String,
    /// 翻译模型
    #[serde(default = "openai_model")]
    pub model: String,
    /// 设为 false 跳过翻译, 直接使用原始识别文本
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for TranslateArgs {
    /// **必须手写**：派生的 `Default` 会把 `bool` 置为 `false`，与 serde 的
    /// `default = "default_true"` 相反。曾导致 `unwrap_or_default()`（配置对象
    /// 整个缺失时）得到 enabled=false，而「对象存在但缺字段」得到 true——
    /// 两条路径语义相反。
    fn default() -> Self {
        Self {
            api_base: openai_base_url(),
            model: openai_model(),
            enabled: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// translate 的**输入契约**：按 `subtitleSource` 分三种来源，每种指向该链
/// fix 步骤落盘的规范化字幕 `srt.json`（路径类型见 `utils::paths` 各命名空间）。
///
/// 设计要点：
/// - **llmFix 不出现在这里**——跑没跑 LLM 修正由产出方消化（fix 步骤最后统一
///   落 srt.json），消费者只认规范化产物。旧的 3×2 焦点文件矩阵由此消失。
/// - **provenance 在类型里**：变体字段是 `paths` 命名空间的类型化路径，
///   不是裸 `PathBuf`。
/// - 未知 `subtitleSource` → 报错而非静默回退（fail-fast）。
#[derive(Debug, Clone)]
pub enum TranslateInput {
    /// `subtitleSource = asr`：读 `asr_fix/srt.json`
    Asr { srt_path: asr_fix::SrtPath },
    /// `subtitleSource = sf_ocr`：读 `sf_ocr_fix/srt.json`
    SfOcr { srt_path: sf_ocr_fix::SrtPath },
    /// `subtitleSource = asr_ocr`：读 `asr_ocr_fix/srt.json`
    AsrOcr { srt_path: asr_ocr_fix::SrtPath },
}

impl TranslateInput {
    /// 从 ctx 解析输入。`subtitleSource` 未知 → 报错（fail-fast）。
    ///
    /// srt.json 的存在性**不在这里检查**——它是否必须存在由
    /// `translate.enabled` 决定（见 `step_translate`：enabled 时缺失报错，
    /// disabled 时缺失走空直通）。
    pub fn from_ctx(ctx: &WorkflowCtx) -> anyhow::Result<Self> {
        let video = PathBuf::from(&ctx.workflow.video_dir);
        // subtitleSource 的解析收拢在 `SubtitleSource::from_ctx`——未知来源在
        // 那里报错，这里是穷尽 match，不需要 error 分支。
        Ok(match SubtitleSource::from_ctx(ctx)? {
            SubtitleSource::Asr => TranslateInput::Asr {
                srt_path: video.join("asr_fix").join("srt.json"),
            },
            SubtitleSource::SfOcr => TranslateInput::SfOcr {
                srt_path: video.join("sf_ocr_fix").join("srt.json"),
            },
            SubtitleSource::AsrOcr => TranslateInput::AsrOcr {
                srt_path: video.join("asr_ocr_fix").join("srt.json"),
            },
        })
    }

    /// 规范化字幕的路径。
    pub fn srt_path(&self) -> &PathBuf {
        match self {
            TranslateInput::Asr { srt_path } | TranslateInput::SfOcr { srt_path }
            | TranslateInput::AsrOcr { srt_path } => srt_path,
        }
    }

    /// 产出方 step 名（报错提示用）。
    pub fn producer(&self) -> &'static str {
        match self {
            TranslateInput::Asr { .. } => "asr_fix",
            TranslateInput::SfOcr { .. } => "sf_ocr_fix",
            TranslateInput::AsrOcr { .. } => "asr_ocr_fix",
        }
    }
}
