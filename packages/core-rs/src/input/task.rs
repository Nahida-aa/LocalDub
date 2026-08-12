use serde::{Deserialize, Serialize};

/// 任务操作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum TaskAction {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "rerun_stage")]
    RerunStage,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "get_group_list")]
    GetGroupList,
    #[serde(rename = "get_task_ctx")]
    GetTaskCtx,
}

/// 目标语言 (langList)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TargetLang {
    En,
    Zh,
    Vi,
    Ja,
    Ko,
    Fr,
    De,
    Es,
    Pt,
    Ru,
    Ar,
    Hi,
    Th,
    Id,
    Ms,
    Tl,
    My,
    Km,
    Lo,
    Mn,
    Ne,
    Ur,
    Bn,
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
    MergeAudio,
    MergeVideo,
}

/// 任务模式: dub=配音, subtitle=仅字幕
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Pipeline {
    Dub,
    Subtitle,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::Dub
    }
}

/// 字幕源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum SubtitleSource {
    #[serde(rename = "asr")]
    Asr,
    #[serde(rename = "sf_ocr")]
    SfOcr,
    #[serde(rename = "asr_ocr")]
    AsrOcr,
}

impl Default for SubtitleSource {
    fn default() -> Self {
        Self::Asr
    }
}

/// 任务参数 (镜像 taskArgsSchema)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskArgs {
    /// 任务操作: start=开始, resume=继续, rerun_stage=重新运行某步骤, status=显示状态, get_group_list=列出分组
    pub action: Option<TaskAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    pub url: Option<String>,
    pub source_lang: Option<TargetLang>,
    pub target_lang: Option<TargetLang>,
    /// 继续任务专业参数, 可指定 resumeFrom 从某步骤开始, 不指定则从上次中断的步骤开始
    pub resume_from: Option<StageName>,
    pub task_dir: Option<String>,
    /// rerunStage 专业参数, 指定要重新运行的步骤
    pub stage_name: Option<StageName>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    #[serde(default)]
    pub pipeline: Pipeline,
    /// 字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)
    #[serde(default)]
    pub subtitle_source: SubtitleSource,
    /// 目标步骤, pipeline 跑到此步骤后自动停止, 不指定则跑完所有步骤
    pub target_stage: Option<StageName>,
}

impl Default for TaskArgs {
    fn default() -> Self {
        Self {
            action: None,
            url: None,
            source_lang: None,
            target_lang: None,
            resume_from: None,
            task_dir: None,
            stage_name: None,
            pipeline: Pipeline::default(),
            subtitle_source: SubtitleSource::default(),
            target_stage: None,
        }
    }
}
