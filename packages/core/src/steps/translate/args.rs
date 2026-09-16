use std::path::Path;

use serde::{Deserialize, Serialize};

use config_rs::env::{openai_base_url, openai_model};

use crate::context::WorkflowState;
use crate::steps::utils::paths::{asr_fix, asr_ocr_fix, sf_ocr_fix, SrtJsonPath};
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
/// - **provenance 在类型里**：`source_path` 的每个变体是 `paths` 命名空间的
///   类型化路径，不是裸 `PathBuf`。
/// - 未知 `subtitleSource` → 报错而非静默回退（fail-fast）。
#[derive(Debug, Clone)]
pub struct TranslateInput {
    /// 本次要读的字幕来源（三选一，各带自己的类型化路径）。
    pub source_path: SubtitleSourcePath,
    /// 任务目录。**与 [`TranslateOutput`] 必须同源**——两边都存是为了让契约
    /// 自包含（消费者不必再拿 ctx），但构造时都由 `from_ctx` 从同一处取，
    /// 避免「读的目录」与「写的目录」分叉。
    pub video_dir: String,
    /// 目标语言（`ctx.target_language`）。
    ///
    /// **`Option` 而非必填**：本结构管的是「字幕从哪读」，而读
    /// `asr_fix/srt.json` 与目标语言**无关**——import 阶段、pipeline 中途、
    /// engine 记产物时都可能还没解析出目标语言。若定为必填，`from_ctx` 会在
    /// 这些合法场景下失败（曾如此，7 个测试红）。
    /// 需要目标语言的地方（如 [`TranslateOutput`]）自己要求必填。
    pub target_lang: Option<String>,
}

impl TranslateInput {
    /// 从 ctx 解析输入。`subtitleSource` 未知 / `target_language` 未设置 → 报错
    /// （fail-fast，均不静默回退）。
    ///
    /// srt.json 的存在性**不在这里检查**——它是否必须存在由
    /// `translate.enabled` 决定（见 `step_translate`：enabled 时缺失报错，
    /// disabled 时缺失走空直通）。
    pub fn from_ctx(ctx: &WorkflowState) -> anyhow::Result<Self> {
        // **顺序有讲究**：先解析 subtitleSource，再取 target_language。
        // 反过来的话，配置里同时有非法 subtitleSource 与缺失 target_language 时，
        // 报的是后者——而用户真正写错的是前者，报错会指向错的地方。
        let source_path = match SubtitleSource::from_ctx(ctx)? {
            SubtitleSource::Asr => {
                SubtitleSourcePath::Asr(asr_fix::SrtPath::new(&ctx.workflow.video_dir))
            }
            SubtitleSource::SfOcr => {
                SubtitleSourcePath::SfOcr(sf_ocr_fix::SrtPath::new(&ctx.workflow.video_dir))
            }
            SubtitleSource::AsrOcr => {
                SubtitleSourcePath::AsrOcr(asr_ocr_fix::SrtPath::new(&ctx.workflow.video_dir))
            }
        };
        Ok(Self {
            source_path,
            video_dir: ctx.workflow.video_dir.clone(),
            target_lang: ctx.target_language.clone(),
        })
    }

    /// 规范化字幕的路径。
    ///
    /// 逐支 match：三种 `SrtPath` 是**不同类型**，or-pattern 要求绑定同一类型。
    /// 这正是 provenance 的代价与收益——类型系统会挡住「把 sf_ocr 的路径塞给
    /// asr 变量」。
    pub fn srt_path(&self) -> &Path {
        match &self.source_path {
            SubtitleSourcePath::Asr(p) => p.as_path(),
            SubtitleSourcePath::SfOcr(p) => p.as_path(),
            SubtitleSourcePath::AsrOcr(p) => p.as_path(),
        }
    }

    /// 产出方 step 名（报错提示用）。
    pub fn producer(&self) -> &'static str {
        match self.source_path {
            SubtitleSourcePath::Asr(_) => "asr_fix",
            SubtitleSourcePath::SfOcr(_) => "sf_ocr_fix",
            SubtitleSourcePath::AsrOcr(_) => "asr_ocr_fix",
        }
    }
}
