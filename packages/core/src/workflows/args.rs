use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::r#const::lang::Language;

/// 任务操作 (serde 与 clap 参数值统一为 snake_case)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum WorkflowAction {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "continue")]
    Continue,
    /// 只导入: 下载/拷贝视频 + 探测 + 写 state.json, 不跑 output_mode。
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
    /// snake_case 名（与 serde 表示、\`state.json\` 里的 \`steps[].name\` 一致）。
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
    /// 此前另有一个读 `state.json` 里 `output_mode` 字符串的 `from_state`：那份字符串
    /// 是 import 拍平的副本来源，`input` 是原源，两个来源共存时就得回答"以谁为准"。
    /// 现在只留 `from_input`——副本（`<video_dir>` 下的 state.json）不再参与决策。
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
#[serde(rename_all = "camelCase")]
pub struct WorkflowArgs {
    /// 任务操作: start=开始, continue=继续, status=显示状态, get_group_list=列出分组
    pub action: Option<WorkflowAction>,
    /// 本地文件路径或云端文件 url、youtubeUrl、bilibiliUrl
    pub url: Option<String>,
    pub source_lang: Option<crate::r#const::lang::LangCode>,
    pub target_lang: Option<Language>,
    /// 继续任务专业参数, 可指定 continueFrom 从某步骤开始, 不指定则从上次中断的步骤开始
    pub continue_from: Option<StepName>,
    /// 目标步骤, output_mode 跑到此步骤后自动停止, 不指定则跑完所有步骤
    pub target_step: Option<StepName>,
    pub video_dir: Option<String>,
    /// 队列任务 ID (cancel_queue 指定要取消的队列项)
    pub queue_id: Option<u64>,
    /// rerunStep 专业参数, 指定要重新运行的步骤
    pub step_name: Option<StepName>,
    /// 实验开关: start/continue 走 aa-workflow 引擎——**开发中，暂不可用**
    /// （集成暂停，见 `integration/workflow/mod.rs`；设了会在 ExecPath 分派时
    /// 打印「开发中」并报错）。恢复后：真源 events.jsonl，与 `run_pipeline`
    /// （真源 state.json）二选一。其余 action 忽略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<bool>,
    /// 实验开关: **非 engine** 路径按步骤 DAG 并行执行（目前只有 `sf_ocr_dub`
    /// 有 DAG 定义，见 [`crate::workflows::dag::StepDag`]；其它组合明确报错）。
    /// 与 `engine` 互斥——同设报错（引擎恢复前设 engine 本身就会报「开发中」）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<bool>,
    /// 任务模式, dub 配音, subtitle 仅字幕
    ///
    /// **显式 `rename` 覆盖 `rename_all = "camelCase"`**：本字段的 wire 名沿用
    /// snake_case（`"output_mode"`），与其余 camelCase 字段不一致——这是刻意的，
    /// 因为 state.json 侧的 `output_mode` 字段也已是 snake_case。
    #[serde(default, rename = "output_mode")]
    pub output_mode: OutputMode,
    /// 字幕源: asr (whisper, 默认), sf_ocr (关键帧策略硬字幕提取), asr_ocr (ASR 时序+OCR 文本融合)
    ///
    /// **显式 `rename` 覆盖 `rename_all = "camelCase"`**：wire 名沿用 snake_case
    /// （`"subtitle_source"`），与 `output_mode` 同理——input.json 与 state.json
    /// 侧字段名保持一套风格。
    #[serde(default, rename = "subtitle_source")]
    pub subtitle_source: SubtitleSource,
    /// 是否下载平台自带字幕 (YouTube/Bilibili 的官方/自动字幕)。
    /// 注意: YouTube 现要求 PO token, 无 bgutil 服务时下载会失败 (best-effort, 不阻断主流程)。
    #[serde(default)]
    pub download_subtitles: bool,
}

/// start/continue 的执行路径（由 `engine` / `parallel` 两个实验标志解析）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPath {
    /// aa-workflow 引擎（真源 events.jsonl）。
    Engine,
    /// 非 engine 的 DAG 并行执行器（真源各 `<step>/state.json`，见
    /// [`crate::workflows::parallel`]）。
    Parallel,
    /// 串行 pipeline（`run_pipeline` / `continue_pipeline`）。
    Serial,
}

impl WorkflowArgs {
    /// 解析执行路径；`engine` 与 `parallel` 同设报错——引擎路径本身已按 DAG
    /// 并行，同设只会让语义含糊（parallel 到底有没有生效说不清）。
    pub fn exec_path(&self) -> anyhow::Result<ExecPath> {
        let engine = self.engine.unwrap_or(false);
        let parallel = self.parallel.unwrap_or(false);
        if engine && parallel {
            anyhow::bail!(
                "workflow.engine 与 workflow.parallel 互斥: 引擎路径本身已按 DAG 并行, 请二选一"
            );
        }
        Ok(if engine {
            ExecPath::Engine
        } else if parallel {
            ExecPath::Parallel
        } else {
            ExecPath::Serial
        })
    }
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

    /// `engine` / `parallel` 两个实验标志 → 执行路径；同设必须报错（语义含糊）。
    #[test]
    fn exec_path_resolves_experiment_flags() {
        let wf = |engine: Option<bool>, parallel: Option<bool>| WorkflowArgs {
            engine,
            parallel,
            ..Default::default()
        };

        assert_eq!(wf(None, None).exec_path().unwrap(), ExecPath::Serial);
        assert_eq!(wf(Some(true), None).exec_path().unwrap(), ExecPath::Engine);
        assert_eq!(
            wf(None, Some(true)).exec_path().unwrap(),
            ExecPath::Parallel
        );
        // 同设报错，错误要点名两个字段
        let err = wf(Some(true), Some(true)).exec_path().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("engine") && msg.contains("parallel") && msg.contains("互斥"),
            "错误应说明互斥, 实际: {msg}"
        );
    }
}
