//! translate 阶段输出结构 (镜像 TS `packages/core/steps/05_translate/out.ts`)。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::context::WorkflowCtx;
use crate::steps::utils::paths::translate::LangPath;

/// 单条翻译段 (镜像 TS `TranslateSegment`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateSegment {
    /// 原文 (识别文本)
    pub text: String,
    /// 译文 (dubbed / subtitled)
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_lang: Option<String>,
    /// 段起点 (ms)
    pub start_ms: u32,
    /// 段终点 (ms)
    pub end_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// `translate/translation.{lang}.json` 结构 (镜像 TS `TranslateResult`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResult {
    pub segments: Vec<TranslateSegment>,
    pub meta: TranslateResultMeta,
}

/// translate 结果 meta (镜像 TS `TranslateResultMeta`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResultMeta {
    pub src_lang: String,
    pub target_lang: String,
}

/// 增量进度文件 `translate/translation.{lang}.partial.json`。
///
/// 每完成一个 batch 即落盘, 记录已翻译的句子 (含缺失标注) 与已完成 batch 索引,
/// 用于: (1) 阶段内续跑时跳过已完成 batch; (2) 把"不等的结果" (缺失句) 也存下来方便分析。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslatePartialResult {
    /// 与最终 `translation.{lang}.json` 同结构的段数组 (缺失句 dst 为空 + missing=true)
    pub segments: Vec<TranslatePartialSegment>,
    /// 已完成 (全部句翻译成功) 的 batch 索引集合
    #[serde(default)]
    pub completed_batches: Vec<usize>,
    pub meta: TranslateResultMeta,
}

/// partial 单段: 在 [`TranslateSegment`] 基础上加 batch 索引与缺失标记。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslatePartialSegment {
    pub text: String,
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_lang: Option<String>,
    pub start_ms: u32,
    pub end_ms: u32,
    /// 所属 batch 索引 (0-based)
    #[serde(default)]
    pub batch_index: usize,
    /// 该句是否缺失 (翻译失败/为空)
    #[serde(default)]
    pub missing: bool,
}

/// translate 的**输出契约**：某个目标语言的译文文件位置。
///
/// 存在的意义是**单点构造**。此前 `translation_file_path(video_dir, lang)` 被 5 处
/// 独立调用（写侧、split_audio 读、mix_video 读、engine 事件登记，以及两处「仅为拼
/// 错误信息」）。`translate/` 目录名与 `translation.{lang}.json` 模板因此散落各处：
/// 改产物命名要同时改 5 处，**漏一处是运行时「文件不存在」，编译期无感**。
///
/// 现在 `translation_file_path` 只在本类型的构造里调用，消费点接收本类型。
///
/// 另一个被顺带修掉的问题：`mix_video` 曾用 `resolve_language()` 现算的 lang 拼
/// 错误信息、却用 `read_translation_result(ctx)` 里 `ctx.target_language` 的快照去读，
/// 两个来源不同（详见 `from_ctx` 注释）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateOutput {
    lang: String,
    path: LangPath,
}

impl TranslateOutput {
    /// **唯一构造入口**。`translation_file_path` 只应在这里被调用。
    pub fn for_lang(video_dir: &str, lang: &str) -> Self {
        Self {
            lang: lang.to_string(),
            path: crate::steps::utils::translation_file_path(video_dir, lang),
        }
    }

    /// 从 ctx 构造，目标语言取 `ctx.target_language`。
    ///
    /// **刻意只用 `ctx.target_language`**，不调 `resolve_language`：后者会重新推断
    /// 并写回 ctx.json，与「读磁盘上那份 ctx.json 快照」可能得到不同答案。本类型
    /// 承诺的是**读与报错指向同一个文件**，故必须单源。
    ///
    /// 未设置 → Err（与 `read_translation_result` 同款语义：目标语言未知时
    /// 无从知道该读哪个文件）。
    pub fn from_ctx(ctx: &WorkflowCtx) -> anyhow::Result<Self> {
        let lang = ctx
            .target_language
            .clone()
            .ok_or_else(|| anyhow::anyhow!("ctx.target_language 未设置, 无法定位翻译结果"))?;
        Ok(Self::for_lang(&ctx.workflow.video_dir, &lang))
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }
}

/// 输出路径的字符串形式——让 `format!("…失败: {out}")` 直接可用，
/// 不必到处 `.path().display()`。
impl std::fmt::Display for TranslateOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())
    }
}
