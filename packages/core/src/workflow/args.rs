use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::r#const::lang::Language;

/// 任务操作 (serde 与 clap 参数值统一为 snake_case)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum WorkflowAction {
    /// 导入视频 → 从头跑全量 pipeline (原 start 的语义)。import 在 workflow 之外。
    #[serde(rename = "import_and_start")]
    ImportAndStart,
    /// 对已导入目录的全量重跑（不含 import）——命令面 `command=workflow --action start`。
    /// 同时作为 `WorkflowInput.workflow.action` 上的 run-kind 标记供步骤读取
    /// （如 tts 用它区分「首次全量」与「续跑」以决定是否忽略 regenIndices）。
    #[serde(rename = "start")]
    Start,
    /// 续跑（setCtx + DagPlan 从源 run 复用未失效步骤）——命令面 `command=workflow --action resume`。
    #[serde(rename = "resume")]
    Resume,
    /// 只导入: 下载/拷贝视频 + 探测 + 写 view.json, 不跑 output_mode。
    /// 批量场景可先批量导入, 之后用 resume 逐个续跑重活。
    #[serde(rename = "import")]
    Import,
    /// 批量按目录导入并跑: 扫描本地目录顶层视频文件, 每个去重后跑一条
    /// import_and_start（粒度仍是单视频）。与 is_enqueue 正交：本地串行执行或入队。
    #[serde(rename = "import_and_start_by_dir")]
    ImportAndStartByDir,
}

impl WorkflowAction {
    /// 该动作是否可入队（is_enqueue=true 时按此决定是否走 enqueue 路径）。
    /// 与 is_enqueue 正交：同一动作可本地执行也可入队。
    pub const fn enqueueable(self) -> bool {
        matches!(
            self,
            Self::ImportAndStart | Self::Resume | Self::Import | Self::ImportAndStartByDir
        )
    }
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
    PartialOrd,
    Ord,
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
    /// snake_case 名（与 serde 表示、\`view.json\` 里的 \`steps[].name\` 一致）。
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
    /// 与 `StepName::as_str` 同款：strum 的 `IntoStaticStr` 一行委托，让
    /// `final_video_dir(output_mode.as_str(), ..)` 这类拼字符串的地方有名字可读。
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// 从 `input.workflow.output_mode` 解析（每一步都从这里取，来源只有一个）。
    ///
    /// 读的是 `Input` 里**结构化**的 `output_mode` 字段——它已是 `OutputMode`
    /// 枚举，不可能出现「未知值」，故返回 `Self` 而非 `Result`；`workflow` 缺失或
    /// 字段缺省时回退 `#[default]`（Dub）。
    ///
    /// 直接吃 `&Input`（而非 `&WorkflowState`），与「from_input」语义一致——只关心输入契约。
    ///
    /// 此前另有一个读 `view.json` 里 `output_mode` 字符串的 `from_state`：那份字符串
    /// 是 import 拍平的副本来源，`input` 是原源，两个来源共存时就得回答"以谁为准"。
    /// 现在只留 `from_input`——副本（`<video_dir>` 下的 view.json）不再参与决策。
    pub fn from_input(input: &crate::input::Input) -> Self {
        input
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

    /// 从 ctx 解析 `input.workflow.subtitle_source`。
    ///
    /// `ctx.input` 已是结构化 [`Input`](crate::input::Input)（import 落定），
    /// `subtitle_source` 为枚举值，不可能出现「未知来源」——该 fail-fast 已上移到
    /// `Input` 反序列化阶段（CLI/RPC 请求解析）。此处仅在 `workflow` 缺失时回退
    /// 默认 `Asr`（`#[default]`），保持与原 `unwrap_or("asr")` 一致。
    pub fn from_input(input: &crate::input::Input) -> anyhow::Result<Self> {
        Ok(input
            .workflow
            .as_ref()
            .map(|w| w.subtitle_source)
            .unwrap_or_default())
    }
}

/// workflow 参数 (镜像 workflowArgsSchema)
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowArgs {
    /// 任务操作: start=开始, resume=续跑, status=显示状态, get_group_list=列出分组
    pub action: Option<WorkflowAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    pub url: Option<String>,
    pub source_lang: Option<crate::r#const::lang::LangStr>,
    pub target_lang: Option<Language>,
    /// 续跑起点集：每个起点 + 其传递下游全部重跑（多点取并集）。
    /// 空 = 跳过已完成前缀，从第一个未完成步骤续跑。对齐引擎 DagPlan.from。
    #[serde(default)]
    pub resume_from: Vec<StepName>,
    /// 目标步骤集：所有目标都达成才停（等价于 dag 中间件的 goals 目标集）。
    /// 空 = 跑全量。对齐引擎 DagPlan.goals。
    #[serde(default)]
    pub target_step: Vec<StepName>,
    pub video_dir: Option<String>,
    /// rerunStep 专业参数, 指定要重新运行的步骤
    pub step_name: Option<StepName>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    #[serde(default)]
    pub output_mode: OutputMode,
    /// 字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)
    #[serde(default)]
    pub subtitle_source: SubtitleSource,
    /// 是否下载平台自带字幕 (YouTube/Bilibili 的官方/自动字幕)。
    /// 注意: YouTube 现要求 PO token, 无 bgutil 服务时下载会失败 (best-effort, 不阻断主流程)。
    #[serde(default)]
    pub download_subtitles: bool,
    /// true = 把 action 交给主服务器队列异步执行（enqueue），false = 本地直接执行。
    /// 与 action 正交：同一个 import_and_start 可本地跑也可入队，不再用 enqueue_* 变体表达。
    #[serde(default)]
    pub is_enqueue: bool,
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
                        serde_json::to_string(&v)
                            .unwrap()
                            .trim_matches('"')
                            .to_string(),
                        v,
                    )
                })
                .collect(),
        );
    }

    #[test]
    fn subtitle_source_serde_and_strum_agree() {
        roundtrip_check(
            [
                SubtitleSource::Asr,
                SubtitleSource::SfOcr,
                SubtitleSource::AsrOcr,
            ]
            .iter()
            .map(|v| {
                (
                    serde_json::to_string(v)
                        .unwrap()
                        .trim_matches('"')
                        .to_string(),
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
                        serde_json::to_string(v)
                            .unwrap()
                            .trim_matches('"')
                            .to_string(),
                        *v,
                    )
                })
                .collect(),
        );
    }

    #[test]
    fn output_mode_from_input_reads_typed_field() {
        use serde_json::json;

        let input = |output_mode: Option<&str>| -> crate::input::Input {
            let v = match output_mode {
                Some(m) => json!({"workflow": {"output_mode": m}}),
                None => json!({}),
            };
            serde_json::from_value(v).unwrap()
        };

        assert_eq!(
            OutputMode::from_input(&input(Some("subtitle"))),
            OutputMode::Subtitle
        );

        // 缺省：input.workflow 缺失也回退默认 Dub
        assert_eq!(OutputMode::from_input(&input(None)), OutputMode::Dub);
    }

}
