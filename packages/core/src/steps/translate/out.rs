//! translate 阶段输出结构 (镜像 TS `packages/core/steps/05_translate/out.ts`)。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::translate::LangPath;

/// 单条翻译段 (镜像 TS `TranslateSegment`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateSegment {
    /// 段序号，**1-based**。
    ///
    /// 此前这个值是隐式的（各处 `enumerate()` 现推），于是每个消费者都要自己
    /// 数一遍——`split_audio` 就重造了一次 `seg_idx`。写进产物后：段的位置成为
    /// **生产时定下的事实**，而不是读的人各自的推断。
    ///
    /// 1-based 与 [`crate::steps::split_audio::out::SplitAudioTiming::seg_idx`]
    /// 对齐（后者一直是 `i + 1`），跨步骤对照同一个段不必换算。
    ///
    /// **必填**（无 `serde(default)`）：旧 `{lang}.json` 缺此字段会反序列化失败，
    /// 提示重跑 translate。刻意不留兼容层——若给默认值 0，「缺失」与「真的第 0 段」
    /// 无法区分，而这个字段的唯一价值就是可被信任。
    pub idx: u32,
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

/// `translate/{lang}.json` 结构 (镜像 TS `TranslateResult`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResult {
    pub segments: Vec<TranslateSegment>,
    pub meta: TranslateResultMeta,
}

impl TranslateResult {
    /// 从 [`LangPath`] 读出**带类型**的结果。
    ///
    /// **为什么读方法放在内容类型上、而不是路径类型上**（按用户决定）：
    /// `LangPath` 只负责「文件在哪」，不该知道「文件里是什么」。类型的知识归
    /// 类型自己——这样 `paths` 模块可以保持对业务类型无知，不产生
    /// `paths ↔ translate::out` 的模块环（`LangPath` 已被 `TranslateOutput` 引用）。
    ///
    /// **为什么返 `Self` 而非 `serde_json::Value`**：此前返 `Value`，于是每个
    /// 消费者都要再手写一遍字段解析（`split_audio` 有 30 行、`mix_video` 有
    /// `extract_segs_from_value`），且每处都用 `unwrap_or(0)` 静默吞错——
    /// 坏数据不会报错，只会变成 0 一路传下去。改成带类型后，解析只有一处，
    /// 且字段缺失/类型不对会在**这里**就失败。
    ///
    /// 与 [`crate::steps::utils::read_timings`] 同款（后者也是返具体类型）。
    pub fn read(path: &LangPath) -> anyhow::Result<Self> {
        let p = path.as_path();
        let raw = std::fs::read_to_string(p)
            .map_err(|e| anyhow::anyhow!("读取翻译产物 {} 失败: {e}", p.display()))?;
        serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析翻译产物 {} 失败: {e}", p.display()))
    }
}

/// translate 结果 meta (镜像 TS `TranslateResultMeta`)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct TranslateResultMeta {
    pub src_lang: String,
    pub target_lang: String,
}

/// 增量进度文件 `translate/{lang}.partial.json`。
///
/// 每完成一个 batch 即落盘, 记录已翻译的句子 (含缺失标注) 与已完成 batch 索引,
/// 用于: (1) 阶段内续跑时跳过已完成 batch; (2) 把"不等的结果" (缺失句) 也存下来方便分析。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslatePartialResult {
    /// 与最终 `{lang}.json` 同结构的段数组 (缺失句 dst 为空 + missing=true)
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
/// 错误信息」）。`translate/` 目录名与 `{lang}.json` 模板因此散落各处：
/// 改产物命名要同时改 5 处，**漏一处是运行时「文件不存在」，编译期无感**。
///
/// 现在 `translation_file_path` 只在本类型的构造里调用，消费点接收本类型。
///
/// 另一个被顺带修掉的问题：`mix_video` 曾用 `resolve_language()` 现算的 lang 拼
/// 错误信息、却用 `read_translation_result(ctx)` 里 `ctx.target_language()` 的快照去读，
/// 两个来源不同（详见上文）。out
/// translate 的**产物事实**：`<translate>/out.json` 的形状。
///
/// 两个字段回答同一件事（这次译到哪、文件叫啥），都写进盘是因为它们**互为校验**：
/// 文件名 `translate/{lang}.json` 里的 lang 就是本次落定的目标语言，下游只需读一次
/// 就能同时定位文件与知道语言，不必再拼一遍命名规则。
///
/// 存在的意义是**单点构造 + 单点事实**。此前 `translation_file_path(video_dir, lang)`
/// 被 5 处独立调用（写侧、split_audio 读、mix_video 读、engine 事件登记，以及两处
/// 「仅为拼错误信息」）；现在拼法收口在 `LangPath::new`，事实收口在本类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TranslateOutput {
    /// 本次落定的目标语言。
    pub target_language: String,
    /// 译文文件路径的自我描述（`translate/{lang}.json`）。
    pub file: std::path::PathBuf,
}

impl TranslateOutput {
    /// **内存构造**（不落盘）：路径拼装委托给 [`LangPath::new`]。
    pub fn for_lang(video_dir: &str, lang: &str) -> Self {
        Self {
            target_language: lang.to_string(),
            file: LangPath::new(video_dir, lang).as_path().to_path_buf(),
        }
    }

    /// **登记**本次产物：`translate` 写出译文文件后调用（唯一写者）。
    ///
    /// 幂等：值相同不重写。
    pub fn write(video_dir: &str, lang: &str) -> anyhow::Result<Self> {
        let out = Self::for_lang(video_dir, lang);
        if Self::read(video_dir)?.as_ref() == Some(&out) {
            return Ok(out);
        }
        crate::state::write_step_out(video_dir, "translate", &out).map_err(anyhow::Error::msg)?;
        tracing::info!(target: "translate", "target_language 已落盘: {lang}");
        Ok(out)
    }

    /// 读 `<translate>/out.json`；还没译完 → `None`。
    pub fn read(video_dir: &str) -> anyhow::Result<Option<Self>> {
        crate::state::read_step_out(video_dir, "translate").map_err(anyhow::Error::msg)
    }

    /// 译文的完整路径。
    ///
    /// 返 `&Path` 而非 `&PathBuf`：调用点一律当路径用（`.display()` / `.parent()` /
    /// `read_to_string` / `write`），没有一处需要 `PathBuf` 的方法。
    pub fn path(&self) -> &Path {
        &self.file
    }

    /// 目标语言（= 落盘事实，非推断）。
    pub fn lang(&self) -> &str {
        &self.target_language
    }
}

/// 输出路径的字符串形式——让 `format!("…失败: {out}")` 直接可用，
/// 不必到处 `.path().display()`。
impl std::fmt::Display for TranslateOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.file.display())
    }
}
