use std::path::Path;

use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

use crate::steps::utils::paths::{asr_fix, asr_ocr_fix, sf_ocr_fix, SrtJsonPath};
use crate::workflows::args::SubtitleSource;

/// translate 阶段参数 (镜像 TS `packages/core/steps/05_translate/args.ts` TranslateArgsSchema)
///
/// 枚举/字符串默认值 TS 在写入 state.json 前已落定 (zod `.prefault({})` / `.default(...)`),
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

impl TranslateArgs {
    /// 从输入契约取本步参数：`input.steps.translate`。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input.steps.translate.clone()
    }
}

fn default_true() -> bool {
    true
}

/// `subtitleSource` 三选一的**类型化字幕路径**。
///
/// 三个变体各自包住该链 fix 步骤的 `SrtPath`（不同型，不可互换）。存在的意义是
/// 让 [`TranslateInput`] 能用**一个字段**表达「本次读哪个来源」——否则三种路径
/// 类型无法共存于同一字段，只能退回「三个变体各写一遍」。
///
/// 放在这里而不是 `utils::paths`：`paths` 是**路径词汇表**（每个类型对应一个
/// 文件/目录），而本枚举是**业务聚合**（「按 subtitleSource 三选一」），归属
/// 在用到它的契约旁边。
#[derive(Debug, Clone)]
pub enum SubtitleSourcePath {
    /// `subtitleSource = asr`：`asr_fix/srt.json`
    Asr(asr_fix::SrtPath),
    /// `subtitleSource = sf_ocr`：`sf_ocr_fix/srt.json`
    SfOcr(sf_ocr_fix::SrtPath),
    /// `subtitleSource = asr_ocr`：`asr_ocr_fix/srt.json`
    AsrOcr(asr_ocr_fix::SrtPath),
}

/// translate 的**输入契约**：这次要读的规范化字幕 + 本次运行的任务级上下文。
///
/// 设计要点：
/// - **llmFix 不出现在这里**——跑没跑 LLM 修正由产出方消化（fix 步骤最后统一
///   落 srt.json），消费者只认规范化产物。旧的 3×2 焦点文件矩阵由此消失。
/// - **provenance 在类型里**：`srt_json_path` 的每个变体是 `paths` 命名空间的
///   类型化路径，不是裸 `PathBuf`。
/// - 未知 `subtitleSource` → 报错而非静默回退（fail-fast）。
#[derive(Debug, Clone)]
pub struct TranslateInput {
    /// 本次要读的字幕来源（三选一，各带自己的类型化路径）。
    pub srt_json_path: SubtitleSourcePath,
    /// 任务目录。**与 [`TranslateOutput`] 必须同源**——两边都存是为了让契约
    /// 自包含，构造时都来自 `new` 的同一个 `video_dir`，避免「读的目录」与
    /// 「写的目录」分叉。
    pub video_dir: String,
    /// 本次运行的参数（`input.steps.translate`）。
    pub args: TranslateArgs,
    /// 本次的**源语言**（resolve_language 的推断结果，非落定事实——它可重算）。
    pub src_lang: String,
    /// 本次目标语言的**推断结果**（DOCTYPE 见 [`TargetLang`]）。
    ///
    /// 与「已落定的事实」是两件事：事实由 translate 写出、供下游定位产物文件名，
    /// 走 `LangPath`；这里只是**本次要翻成什么**（配置 + 源语言推断），translate
    /// 跑完后才结为事实。
    pub target_lang: Option<String>,
}

impl TranslateInput {
    /// `(video_dir, input)` → 输入契约。`subtitleSource` 未知 → 报错（fail-fast）。
    ///
    /// srt.json 的存在性**不在这里检查**——它是否必须存在由
    /// `translate.enabled` 决定（见 `step_translate`：enabled 时缺失报错，
    /// disabled 时缺失走空直通）。
    pub fn new(video_dir: &str, input: &crate::input::Input) -> anyhow::Result<Self> {
        // **顺序有讲究**：先解析 subtitleSource，再算目标语言。
        // 反过来的话，配置了非法 subtitleSource 时暴露的可能是后者——而用户真正
        // 写错的是前者，报错会指向错的地方。
        let srt_json_path = match SubtitleSource::from_input(input)? {
            SubtitleSource::Asr => SubtitleSourcePath::Asr(asr_fix::SrtPath::new(video_dir)),
            SubtitleSource::SfOcr => SubtitleSourcePath::SfOcr(sf_ocr_fix::SrtPath::new(video_dir)),
            SubtitleSource::AsrOcr => {
                SubtitleSourcePath::AsrOcr(asr_ocr_fix::SrtPath::new(video_dir))
            }
        };
        let (src_lang, target) =
            crate::steps::utils::resolve_language(video_dir, input).map_err(anyhow::Error::msg)?;
        Ok(Self {
            srt_json_path,
            video_dir: video_dir.to_string(),
            args: TranslateArgs::from_input(input),
            src_lang,
            target_lang: Some(target),
        })
    }

    /// 规范化字幕的路径。
    ///
    /// 逐支 match：三种 `SrtPath` 是**不同类型**，or-pattern 要求绑定同一类型。
    /// 这正是 provenance 的代价与收益——类型系统会挡住「把 sf_ocr 的路径塞给
    /// asr 变量」。
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
