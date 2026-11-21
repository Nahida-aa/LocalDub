use std::path::Path;

use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

use crate::r#const::lang::LangCode;
use crate::state::lang::{resolve_source_language, resolve_target_language};
use crate::steps::utils::paths::{asr_fix, asr_ocr_fix, sf_ocr_fix, SrtJsonPath};
use crate::workflows::args::SubtitleSource;

/// translate 阶段参数 (镜像 TS `packages/core/steps/05_translate/args.ts` TranslateArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})` / `.default(...)`),
/// 这里只需处理「对象存在但字段缺」: 字段级 `#[serde(default…)]` 兜底即可。
///
/// 目标语言统一在 `input.workflow.target_lang` 配置 (任务级概念: 一个任务只有一个翻译
/// 阶段, 不需要 step 级覆盖)。旧的 `steps.translate.target_lang` 已移除,
/// deny_unknown_fields 让残留配置明确报错 (不能静默失效 -> 翻错语言)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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

impl TranslateArgs {
    /// 从输入契约取本步参数：`input.steps.translate`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.translate.clone()
    }
}

fn default_true() -> bool {
    true
}

/// `subtitle_source` 三选一的**类型化字幕路径**。
///
/// 三个变体各自包住该链 fix 步骤的 `SrtPath`（不同型，不可互换）。存在的意义是
/// 让 [`TranslateDeps`] 能用**一个字段**表达「本次读哪个来源」——否则三种路径
/// 类型无法共存于同一字段，只能退回「三个变体各写一遍」。
///
/// 放在这里而不是 `utils::paths`：`paths` 是**路径词汇表**（每个类型对应一个
/// 文件/目录），而本枚举是**业务聚合**（「按 subtitle_source 三选一」），归属
/// 在用到它的契约旁边。
#[derive(Debug, Clone)]
pub enum SubtitleSourcePath {
    /// `subtitle_source = asr`：`asr_fix/srt.json`
    Asr(asr_fix::SrtPath),
    /// `subtitle_source = sf_ocr`：`sf_ocr_fix/srt.json`
    SfOcr(sf_ocr_fix::SrtPath),
    /// `subtitle_source = asr_ocr`：`asr_ocr_fix/srt.json`
    AsrOcr(asr_ocr_fix::SrtPath),
}


/// Phase 1 事实面：`step_translate` 的显式依赖清单（由装配层算好后注入）。
///
/// - `srt_json_path`：上游 fix 步骤产出的规范化字幕（代表的是**上游产物**，
///   input 决定不了哪个产出方、产物在不在）；
/// - `src_lang` / `target_lang`：装配层按 input 配置 + `<asr>/out.json` 实测
///   算好的本次语言决策（三级回退 + 直通），步骤体只消费。
///
/// `LangCode` 是开放字符串 newtype：既容纳 `Language` 枚举的所有值，也容纳
/// 直通时可能与 src 相同的列表外语言码（如 `it`），与下游 `String` 同源。
#[derive(Debug, Clone)]
pub struct TranslateDeps {
    pub srt_json_path: SubtitleSourcePath,
    pub src_lang: LangCode,
    pub target_lang: LangCode,
}

impl TranslateDeps {
    /// 装配层构造：从 workflow input 取任务目录，按 `subtitle_source` 选 SrtPath，
    /// 并读一次 `<asr>/out.json` 的实测语言。`WorkflowInput` 已保证 video_dir /
    /// subtitle_source 落定，故此处只做路径构造与磁盘事实读取，无任何校验分支。
    pub fn new(input: &crate::input::WorkflowInput) -> Self {
        let video_dir = input.video_dir();
        let srt_json_path = match input.workflow.subtitle_source {
            SubtitleSource::Asr => SubtitleSourcePath::Asr(asr_fix::SrtPath::new(video_dir)),
            SubtitleSource::SfOcr => SubtitleSourcePath::SfOcr(sf_ocr_fix::SrtPath::new(video_dir)),
            SubtitleSource::AsrOcr => {
                SubtitleSourcePath::AsrOcr(asr_ocr_fix::SrtPath::new(video_dir))
            }
        };
        let asr_language = crate::steps::asr::out::asr_language(video_dir);
        // "从什么翻到什么"在装配层算好（与现役 resolve_language 同款三级回退 + 直通），
        // 步骤体只消费——语言决策是 run 级事实，不是步骤内部现算。
        let src_lang = resolve_source_language(
            asr_language.as_ref(),
            input.workflow.source_lang.as_ref(),
        );
        let target_lang = if !input.steps.translate.enabled {
            src_lang.clone()
        } else {
            LangCode::from(resolve_target_language(&src_lang, input.workflow.target_lang))
        };
        Self {
            srt_json_path,
            src_lang,
            target_lang,
        }
    }

    /// 规范化字幕路径（与现役 `TranslateInput::srt_path` 同款逐支 match）。
    pub fn srt_path(&self) -> &Path {
        match &self.srt_json_path {
            SubtitleSourcePath::Asr(p) => p.as_path(),
            SubtitleSourcePath::SfOcr(p) => p.as_path(),
            SubtitleSourcePath::AsrOcr(p) => p.as_path(),
        }
    }

    /// 产出方 step 名（报错提示用）。
    pub fn producer(&self) -> &'static str {
        match self.srt_json_path {
            SubtitleSourcePath::Asr(_) => "asr_fix",
            SubtitleSourcePath::SfOcr(_) => "sf_ocr_fix",
            SubtitleSourcePath::AsrOcr(_) => "asr_ocr_fix",
        }
    }
}
