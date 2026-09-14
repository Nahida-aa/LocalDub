use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::r#const::lang::TargetLang;

/// 任务操作 (serde 与 clap 参数值统一为 snake_case)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum WorkflowAction {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "continue")]
    Continue,
    /// 只导入: 下载/拷贝视频 + 探测 + 写 ctx.json, 不跑 pipeline。
    /// 批量场景可先批量导入, 之后用 continue 逐个续跑重活。
    #[serde(rename = "import")]
    Import,
    #[serde(rename = "enqueue_start")]
    EnqueueStart,
    #[serde(rename = "enqueue_continue")]
    EnqueueContinue,
    #[serde(rename = "enqueue_import")]
    EnqueueImport,
    /// 批量入队目录: 扫描本地目录顶层视频文件, 每个去重后入队一条 action=start 任务。
    /// (目录是入队动作的入参, 队列粒度仍是单视频 start 事件。)
    #[serde(rename = "enqueue_dir")]
    EnqueueDir,
    #[serde(rename = "list_queue")]
    ListQueue,
    #[serde(rename = "cancel_queue")]
    CancelQueue,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "get_group_list")]
    GetGroupList,
    #[serde(rename = "get_workflow_ctx")]
    GetWorkflowCtx,
    #[serde(rename = "generate_meta")]
    GenerateMeta,
}

/// pipeline 阶段名。
///
/// **这是 step 名单的唯一真源**：`STEPS_LIST`、`has_handler`、`run_step` 的分派
/// 都从它派生，这样改名漏一处会**编译失败**，而不是静默跳过。
///
/// 每个变体对应一个 `steps/<name>/` 目录与一个 `run_step` 分支；读/写契约见
/// `steps/DEPENDENCIES.md`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum StepName {
    Separate,
    SeparateAfter,
    Asr,
    AsrFix,
    SfOcrPre,
    SfOcr,
    SfOcrFix,
    AsrOcrPre,
    AsrOcr,
    AsrOcrFix,
    Translate,
    SplitAudio,
    Tts,
    MixAudio,
    MixVideo,
}

/// 全部 step，**顺序与 `STEPS_LIST` 的历史顺序一致**（供校验用；pipeline 序列
/// 本身定义在 `steps::utils::steps` 的 `*_STEPS` 常量里）。
pub const ALL_STEPS: &[StepName] = &[
    StepName::Separate,
    StepName::SeparateAfter,
    StepName::Asr,
    StepName::AsrFix,
    StepName::SfOcrPre,
    StepName::SfOcr,
    StepName::SfOcrFix,
    StepName::AsrOcrPre,
    StepName::AsrOcr,
    StepName::AsrOcrFix,
    StepName::Translate,
    StepName::SplitAudio,
    StepName::Tts,
    StepName::MixAudio,
    StepName::MixVideo,
];

impl StepName {
    /// snake_case 名（与 serde 表示、`ctx.json` 里的 `steps[].name` 一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            StepName::Separate => "separate",
            StepName::SeparateAfter => "separate_after",
            StepName::Asr => "asr",
            StepName::AsrFix => "asr_fix",
            StepName::SfOcrPre => "sf_ocr_pre",
            StepName::SfOcr => "sf_ocr",
            StepName::SfOcrFix => "sf_ocr_fix",
            StepName::AsrOcrPre => "asr_ocr_pre",
            StepName::AsrOcr => "asr_ocr",
            StepName::AsrOcrFix => "asr_ocr_fix",
            StepName::Translate => "translate",
            StepName::SplitAudio => "split_audio",
            StepName::Tts => "tts",
            StepName::MixAudio => "mix_audio",
            StepName::MixVideo => "mix_video",
        }
    }

    /// 解析 step 名。**未知名返回 `None`**——调用方应报错而不是跳过。
    pub fn parse(s: &str) -> Option<Self> {
        ALL_STEPS.iter().copied().find(|x| x.as_str() == s)
    }
}

impl std::fmt::Display for StepName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 任务模式: dub=配音, subtitle=仅字幕
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Pipeline {
    #[default]
    Dub,
    Subtitle,
}

/// 字幕源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub enum SubtitleSource {
    #[default]
    #[serde(rename = "asr")]
    Asr,
    #[serde(rename = "sf_ocr")]
    SfOcr,
    #[serde(rename = "asr_ocr")]
    AsrOcr,
}

/// workflow 参数 (镜像 workflowArgsSchema)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowArgs {
    /// 任务操作: start=开始, continue=继续, status=显示状态, get_group_list=列出分组
    pub action: Option<WorkflowAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    pub url: Option<String>,
    pub source_lang: Option<crate::r#const::lang::Language>,
    pub target_lang: Option<TargetLang>,
    /// 继续任务专业参数, 可指定 continueFrom 从某步骤开始, 不指定则从上次中断的步骤开始
    pub continue_from: Option<StepName>,
    /// 目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤
    pub target_step: Option<StepName>,
    pub video_dir: Option<String>,
    /// 队列任务 ID (cancel_queue 指定要取消的队列项)
    pub queue_id: Option<u64>,
    /// rerunStep 专业参数, 指定要重新运行的步骤
    pub step_name: Option<StepName>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    #[serde(default)]
    pub pipeline: Pipeline,
    /// 字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)
    #[serde(default)]
    pub subtitle_source: SubtitleSource,
    /// 是否下载平台自带字幕 (YouTube/Bilibili 的官方/自动字幕)。
    /// 注意: YouTube 现要求 PO token, 无 bgutil 服务时下载会失败 (best-effort, 不阻断主流程)。
    #[serde(default)]
    pub download_subtitles: bool,
}
