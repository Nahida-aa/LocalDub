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
    /// 只导入: 下载/拷贝视频 + 探测 + 写 ctx.json, 不跑 output_mode。
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

/// output_mode 阶段名。
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
    strum::EnumVariantNames,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Dub,
    Subtitle,
}

impl OutputMode {
    /// 从 ctx 解析 `ctx.output_mode`（import 落定的事实，字符串形式）。
    ///
    /// 与 `SubtitleSource::from_ctx`（读 `input.workflow.subtitleSource` 原始输入）不同，
    /// OutputMode 已被 import 拍平进 `ctx.output_mode`，此处读**事实**而非重解析输入
    /// —— 与 `LangPath::from_ctx`「读事实，缺失即报错」的取向一致。
    ///
    /// 空串按 `#[default]`（Dub）兜底（与 `read_ctx_from_value` / `WorkflowArgs` 默认一致）；
    /// 未知值报错（fail-fast，期望列表由 `VARIANTS` 生成，加变体自动更新）。
    pub fn from_ctx(ctx: &crate::context::WorkflowCtx) -> anyhow::Result<Self> {
        use strum::VariantNames;
        let s = ctx.output_mode.trim();
        if s.is_empty() {
            return Ok(Self::default());
        }
        s.parse::<OutputMode>().map_err(|_| {
            anyhow::anyhow!(
                "未知的 output_mode \"{s}\"（期望 {}）",
                Self::VARIANTS.join(" / ")
            )
        })
    }

    /// 从 `ctx.input.workflow.output_mode` 解析（读**输入契约**，非 import 落定的事实）。
    ///
    /// 与 [`from_ctx`](Self::from_ctx)（读 `ctx.output_mode` 事实、未知值 fail-fast）互补：
    /// 此处读的是 `Input` 里**结构化**的 `output_mode` 字段——它已是 `OutputMode`
    /// 枚举，不可能出现「未知值」，故返回 `Self` 而非 `Result`；`workflow` 缺失或
    /// 字段缺省时回退 `#[default]`（Dub）。
    ///
    /// 适用：需要在 import **之前**、或从原始输入（而非拍平后的事实）取 output_mode 的场景。
    /// 注意 `from_ctx` 与 `from_input` 来源不同——前者是 import 落定的事实，后者是用户传入的输入；
    /// 正常流程里两者应一致，不一致时以 `from_ctx`（事实）为准。
    pub fn from_input(ctx: &crate::context::WorkflowCtx) -> Self {
        ctx.input
            .workflow
            .as_ref()
            .map(|w| w.output_mode)
            .unwrap_or_default()
    }
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

    /// 从 ctx 解析 `input.workflow.subtitleSource`。
    ///
    /// `ctx.input` 已是结构化 [`Input`](crate::input::Input)（import 落定），
    /// `subtitle_source` 为枚举值，不可能出现「未知来源」——该 fail-fast 已上移到
    /// `Input` 反序列化阶段（CLI/RPC 请求解析）。此处仅在 `workflow` 缺失时回退
    /// 默认 `Asr`（`#[default]`），保持与原 `unwrap_or("asr")` 一致。
    pub fn from_ctx(ctx: &crate::context::WorkflowCtx) -> anyhow::Result<Self> {
        Ok(ctx
            .input
            .workflow
            .as_ref()
            .map(|w| w.subtitle_source)
            .unwrap_or_default())
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
    /// 目标步骤, output_mode 跑到此步骤后自动停止, 不指定则跑完所有步骤
    pub target_step: Option<StepName>,
    pub video_dir: Option<String>,
    /// 队列任务 ID (cancel_queue 指定要取消的队列项)
    pub queue_id: Option<u64>,
    /// rerunStep 专业参数, 指定要重新运行的步骤
    pub step_name: Option<StepName>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    ///
    /// **显式 `rename` 覆盖 `rename_all = "camelCase"`**：本字段的 wire 名沿用
    /// snake_case（`"output_mode"`），与其余 camelCase 字段不一致——这是刻意的，
    /// 因为 ctx.json 侧的 `output_mode` 字段也已是 snake_case。
    #[serde(default, rename = "output_mode")]
    pub output_mode: OutputMode,
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
    //! （StepName / SubtitleSource / OutputMode）的 serde↔strum 一致性钉住。

    use super::*;
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
    fn output_mode_serde_and_strum_agree() {
        roundtrip_check(
            [OutputMode::Dub, OutputMode::Subtitle]
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
    fn output_mode_from_ctx_reads_fact() {
        use crate::context::read_ctx_from_value;
        use serde_json::json;

        let mk = |output_mode: Option<&str>| {
            let mut v = json!({
                "workflow": {"id":"t","video_dir":"/w","url":"http://e","source":"remote",
                             "status":"running","created_at":"2024-01-01T00:00:00Z"}
            });
            if let Some(m) = output_mode {
                v["output_mode"] = json!(m);
            }
            read_ctx_from_value(v).unwrap()
        };

        // 读事实：ctx.output_mode 已是 import 落定的值
        assert_eq!(
            OutputMode::from_ctx(&mk(Some("subtitle"))).unwrap(),
            OutputMode::Subtitle
        );

        // 缺省：read_ctx_from_value 默认 dub，from_ctx 解析出 Dub
        assert_eq!(mk(None).output_mode, "dub");
        assert_eq!(OutputMode::from_ctx(&mk(None)).unwrap(), OutputMode::Dub);

        // 未知值报错（fail-fast）
        assert!(OutputMode::from_ctx(&mk(Some("karaoke"))).is_err());
    }

    #[test]
    fn output_mode_from_input_reads_typed_field() {
        use crate::context::read_ctx_from_value;
        use serde_json::json;

        let mk = |output_mode: Option<&str>| {
            let mut v = json!({
                "workflow": {"id":"t","video_dir":"/w","url":"http://e","source":"remote",
                             "status":"running","created_at":"2024-01-01T00:00:00Z"}
            });
            if let Some(m) = output_mode {
                v["input"] = json!({"workflow": {"output_mode": m}});
            }
            read_ctx_from_value(v).unwrap()
        };

        // 读结构化输入字段（与 from_ctx 来源不同：这里是 input，不是拍平事实）
        assert_eq!(
            OutputMode::from_input(&mk(Some("subtitle"))),
            OutputMode::Subtitle
        );

        // 缺省：input.workflow 缺失也回退默认 Dub
        assert_eq!(OutputMode::from_input(&mk(None)), OutputMode::Dub);
    }
}
