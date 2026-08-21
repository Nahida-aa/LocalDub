use serde::{Deserialize, Serialize};

use crate::r#const::lang::TargetLang;

/// 任务操作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum TaskAction {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "enqueue_start")]
    EnqueueStart,
    #[serde(rename = "enqueue_continue")]
    EnqueueContinue,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "get_group_list")]
    GetGroupList,
    #[serde(rename = "get_task_ctx")]
    GetTaskCtx,
    #[serde(rename = "generate_meta")]
    GenerateMeta,
}

/// pipeline 阶段名 (stagesList)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
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

/// 任务参数 (镜像 taskArgsSchema)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskArgs {
    /// 任务操作: start=开始, continue=继续, status=显示状态, get_group_list=列出分组
    pub action: Option<TaskAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    pub url: Option<String>,
    pub source_lang: Option<TargetLang>,
    pub target_lang: Option<TargetLang>,
    /// 继续任务专业参数, 可指定 continueFrom 从某步骤开始, 不指定则从上次中断的步骤开始
    pub continue_from: Option<StageName>,
    /// 目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤
    pub target_stage: Option<StageName>,
    pub task_dir: Option<String>,
    /// rerunStage 专业参数, 指定要重新运行的步骤
    pub stage_name: Option<StageName>,
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
