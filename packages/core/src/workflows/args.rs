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
///
/// `as_str` / `FromStr` / 全量名单（`EnumIter`）由 strum 派生——serde 的
/// `rename_all` 与 strum 的 `serialize_all` 必须一致，有 roundtrip 测试钉住。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    specta::Type,
    ValueEnum,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::EnumIter,
    strum::EnumVariantNames,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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

impl StepName {
    /// snake_case 名（与 serde 表示、\`ctx.json\` 里的 \`steps[].name\` 一致）。
    /// 一行委托到 strum 派生的 \`AsRef<str>\`——名字承载契约语义，保留专属方法。
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}


/// 任务模式: dub=配音, subtitle=仅字幕
///
/// `Display` / `FromStr` 由 strum 派生（serialize_all = lowercase，与 serde 一致）。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    specta::Type,
    strum::Display,
    strum::EnumString,
    strum::AsRefStr,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Pipeline {
    #[default]
    Dub,
    Subtitle,
}

/// 字幕源
///
/// `as_str` / `FromStr` / `Display` 由 strum 派生（serialize_all = snake_case，
/// 与 serde rename 一致，roundtrip 测试钉住）。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    specta::Type,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::EnumVariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SubtitleSource {
    #[default]
    #[serde(rename = "asr")]
    Asr,
    #[serde(rename = "sf_ocr")]
    SfOcr,
    #[serde(rename = "asr_ocr")]
    AsrOcr,
}

impl SubtitleSource {
    /// snake_case 名（与 serde 表示、input.jsonc 里的字符串一致）。
    /// 一行委托到 strum 派生的 `AsRef<str>`。
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// 从 ctx 解析 `input.workflow.subtitleSource`。未知名 → 报错（fail-fast；
    /// 缺省时按 `#[default]` 走 asr）。期望值列表从 `VARIANTS` 生成——加变体
    /// 自动更新，不会漏。
    pub fn from_ctx(ctx: &crate::context::WorkflowCtx) -> anyhow::Result<Self> {
        use strum::VariantNames;
        let s = ctx
            .input
            .get("workflow")
            .and_then(|v| v.get("subtitleSource"))
            .and_then(|v| v.as_str())
            .unwrap_or("asr");
        s.parse::<SubtitleSource>().map_err(|_| {
            anyhow::anyhow!(
                "未知的 subtitleSource \"{s}\"（期望 {}）",
                Self::VARIANTS.join(" / ")
            )
        })
    }
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

#[cfg(test)]
mod strum_sync_tests {
    //! serde 的 `rename_all` 与 strum 的 `serialize_all` 是**两套独立属性**——
    //! 不同步就是「JSON 里的名字」与「解析接受的名字」静默分叉。本测试把三者
    //! （StepName / SubtitleSource / Pipeline）的 serde↔strum 一致性钉住。

    use super::*;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    fn roundtrip_check<T>(variants: Vec<(String, T)>)
    where
        T: PartialEq + std::fmt::Debug + Into<&'static str> + std::str::FromStr,
        <T as std::str::FromStr>::Err: std::fmt::Display,
    {
        for (json, v) in variants {
            let parsed: T = json
                .parse()
                .unwrap_or_else(|e| panic!("\"{json}\" 应可被 FromStr 解析: {e}"));
            assert_eq!(parsed, v);
        }
    }

    #[test]
    fn step_name_serde_and_strum_agree() {
        roundtrip_check(
            StepName::iter()
                .map(|v| {
                    (
                        serde_json::to_string(&v).unwrap().trim_matches('"').to_string(),
                        v,
                    )
                })
                .collect(),
        );
    }

    #[test]
    fn subtitle_source_serde_and_strum_agree() {
        roundtrip_check(
            [SubtitleSource::Asr, SubtitleSource::SfOcr, SubtitleSource::AsrOcr]
                .iter()
                .map(|v| {
                    (
                        serde_json::to_string(v).unwrap().trim_matches('"').to_string(),
                        *v,
                    )
                })
                .collect(),
        );
    }

    #[test]
    fn pipeline_serde_and_strum_agree() {
        roundtrip_check(
            [Pipeline::Dub, Pipeline::Subtitle]
                .iter()
                .map(|v| {
                    (
                        serde_json::to_string(v).unwrap().trim_matches('"').to_string(),
                        *v,
                    )
                })
                .collect(),
        );
    }
}
