//! asr_ocr_pre 步骤的输出 DTO（产物 `asr_ocr_pre/asr_split.json`）。
//!
//! 产出方 `pre.rs` 写、消费方 `fix.rs`（asr_ocr_fix）读，共用同一份类型定义，
//! 避免像此前那样在两边各写一遍 `AsrSplitResult`（字段一旦漂移两端互不可见）。
//! 镜像 `asr/out.rs` 把「步骤产物类型」从步骤逻辑里抽出来单独成文件。

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use subtitle::SubtitleSegment;

/// asr_ocr_pre 步骤产出的 ASR 切分结果（`asr_ocr_pre/asr_split.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrSplitResult {
    pub result: AsrSplitResultBody,
    pub meta: AsrSplitResultMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrSplitResultBody {
    pub text: String,
    pub segments: Vec<SubtitleSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrSplitResultMeta {
    /// 切分前的原始 ASR 段数（来自 `asr/asr.json`）。
    ///
    /// 下游 `asr_ocr_fix` 直接把它当 `asr_raw_len` 用（fusion 遥测 / 日志），所以本步
    /// 写它时顺手记下，下游无需回读 `asr/asr.json`——这也是 `AsrOcrFixInput` 能去掉
    /// `asr_path` 依赖的前提。
    pub original_segments_count: usize,
    /// 切分后的段数。
    pub segments_count: usize,
}

impl AsrSplitResult {
    /// 读取 `asr_ocr_pre/asr_split.json` 并反序列化。
    ///
    /// 消费方（asr_ocr_fix）统一走这里，路径约定由调用方经
    /// [`crate::steps::utils::paths::asr_ocr_pre::AsrSplitPath`] 提供。
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("读取 {} 失败", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("解析 {} 失败", path.display()))
    }
}
