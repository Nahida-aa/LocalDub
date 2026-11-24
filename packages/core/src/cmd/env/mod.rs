//! 环境检测命令 (`cli env [check|ensure] [targets...]`)。
//!
//! 镜像 TS `packages/core/cmd/env/index.ts`: `runCheck` / `runEnsure` /
//! `formatResult` + `resolveTargets`。检查逻辑见 `items.rs`, 元信息见 `input.rs`。

pub mod args;
pub mod handler;
pub mod input;
pub mod items;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::cmd::env::input::EnvKey;
use crate::input::Input;

/// 检查状态 (serde 小写对齐 TS 字符串 "pass"/"warn"/"fail"/"skip")。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

/// 单项检查结果 (对齐 TS `CheckResult`)。
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub key: String,
    pub status: CheckStatus,
    /// 容纳 version/size/issues/missing_bins/stale_bins/path 等自由字段。
    pub data: serde_json::Value,
    pub required: bool,
}

/// 全部环境项 key 集合 (镜像 TS `envList`，字符串形式供 CLI/server 展示)。
pub fn env_list() -> Vec<&'static str> {
    EnvKey::all().iter().map(|k| k.as_str()).collect()
}

/// 按 `input.jsonc` 的 steps 配置推断本次任务所需的环境项 (targets 为空时使用)。
///
/// 设计: 从「核心工具链 + 配置」基础集出发, 按各 step 的 runtime/device 追加依赖。
/// 这是启发式推断 (非精确依赖图), 目标是给出「跟当前配置相关」的检查项, 避免每次扫全量。
pub fn infer_targets(input: &Input) -> (Vec<EnvKey>, HashMap<EnvKey, String>) {
    use crate::steps::asr::args::{AsrDevice, AsrRuntime};
    use crate::steps::separate::args::Device as SepDevice;
    use crate::steps::tts::args::{TtsDevice, TtsRuntime};
    use crate::workflow::args::SubtitleSource;

    let mut set: HashSet<EnvKey> = HashSet::new();
    let mut desired: HashMap<EnvKey, String> = HashMap::new();
    let add = |k: EnvKey, set: &mut HashSet<EnvKey>| {
        set.insert(k);
    };

    // 基础: 任何任务都需要的工具链与配置
    for k in [
        EnvKey::Bun,
        EnvKey::Python,
        EnvKey::Uv,
        EnvKey::Ffmpeg,
        EnvKey::Git,
        EnvKey::Cmake,
        EnvKey::Dotenv,
    ] {
        add(k, &mut set);
    }

    // --- import: 平台视频链接 (YouTube/Bilibili) 需要 yt-dlp ---
    if let Some(url) = input.workflow.as_ref().and_then(|w| w.url.as_deref()) {
        if crate::workflow::import::util::is_youtube_url(url)
            || crate::workflow::import::util::is_bilibili_url(url)
        {
            add(EnvKey::YtDlp, &mut set);
        }
    }

    let steps = &input.steps;
    let subtitle_source = input
        .workflow
        .as_ref()
        .map(|t| t.subtitle_source)
        .unwrap_or(SubtitleSource::Asr);

    // --- sf_ocr: 关键帧筛选 + 逐帧 OCR + 后处理均走 vision-lab release 二进制 ---
    if subtitle_source == SubtitleSource::SfOcr {
        add(EnvKey::SubtitleFinderBin, &mut set);
        add(EnvKey::SubtitleOcrBin, &mut set);
        add(EnvKey::SubtitleOcrPostBin, &mut set);
    }
    // asr_ocr 阶段依赖 subtitle_ocr_bin
    if subtitle_source != SubtitleSource::Asr {
        add(EnvKey::SubtitleOcrBin, &mut set);
    }

    // --- asr ---
    match steps.asr.runtime {
        AsrRuntime::Ggml | AsrRuntime::FasterWhisper | AsrRuntime::Pytorch => {
            add(EnvKey::WhisperGgml, &mut set)
        }
    }
    if steps.asr.vad {
        add(EnvKey::WhisperVad, &mut set);
    }
    if steps.asr.device == AsrDevice::Vulkan {
        add(EnvKey::WhisperBin, &mut set);
        add(EnvKey::Vulkan, &mut set);
    }
    if steps.asr.device == AsrDevice::Cuda {
        add(EnvKey::Cuda, &mut set);
    }
    if steps.asr.device == AsrDevice::Mps {
        add(EnvKey::Cuda, &mut set); // mps 复用 apple 驱动检查 (无独立项)
    }

    // --- separate / demucs ---
    if steps.asr.use_separated || steps.separate.always {
        add(EnvKey::DemucsPth, &mut set);
        // demucs-burn 已迁至 vox-lab: 仅 tch/wgpu 有 release 资产
        let bin = steps.separate.runtime.resolve(steps.separate.device);
        match bin.env_key {
            Some(key) => add(key, &mut set),
            None => {
                add(EnvKey::DemucsBurnBin, &mut set);
                desired.insert(EnvKey::DemucsBurnBin, bin.suffix.to_string());
            }
        }
        match steps.separate.device {
            SepDevice::Vulkan => add(EnvKey::Vulkan, &mut set),
            SepDevice::Cuda => add(EnvKey::Cuda, &mut set),
            SepDevice::Webgpu => add(EnvKey::Vulkan, &mut set), // webgpu 走 vulkan 驱动
            SepDevice::Cpu | SepDevice::Mps => {}
        }
    }

    // --- translate ---
    if steps.translate.enabled {
        add(EnvKey::Openai, &mut set);
    }

    // --- tts ---
    match steps.tts.runtime {
        TtsRuntime::Cloud => add(EnvKey::Openai, &mut set),
        TtsRuntime::Ggml => add(EnvKey::Voxcpm2Onnx, &mut set),
        TtsRuntime::VoxcpmTorchGradio => add(EnvKey::Voxcpm2Pth, &mut set),
    }
    match steps.tts.device {
        TtsDevice::Webgpu => add(EnvKey::Vulkan, &mut set),
        TtsDevice::Cuda => add(EnvKey::Cuda, &mut set),
        TtsDevice::Rocm => add(EnvKey::Rocm, &mut set),
        TtsDevice::Cpu | TtsDevice::Mps => {}
    }

    // 按 EnvKey 声明顺序稳定输出 (与 run_check 全量顺序一致)
    let targets = EnvKey::all()
        .into_iter()
        .filter(|k| set.contains(k))
        .collect();
    (targets, desired)
}

/// 确保指定的环境项就绪 (幂等: 已存在直接返回路径; 否则 check 不通过则 ensure)。
///
/// 供阶段消费方调用, 替代 `find_release_bin` + `cargo_build_bin`。
pub async fn ensure_bin(key: EnvKey) -> anyhow::Result<PathBuf> {
    let check_result = key.check(None).await;
    if check_result.status == CheckStatus::Pass {
        return key
            .bin_path()
            .ok_or_else(|| anyhow::anyhow!("环境项 {} 无二进制路径", key.as_str()));
    }
    let ensure_result = key
        .ensure()
        .await
        .ok_or_else(|| anyhow::anyhow!("环境项 {} 无 ensure 实现", key.as_str()))?;
    if ensure_result.status != CheckStatus::Pass {
        return Err(anyhow::anyhow!(
            "ensure {} 失败: {}",
            key.as_str(),
            ensure_result
                .data
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误")
        ));
    }
    key.bin_path()
        .ok_or_else(|| anyhow::anyhow!("环境项 {} 无二进制路径", key.as_str()))
}

/// 确保二进制就绪，返回**已配好运行时环境**的 [`Command`]。
///
/// release zip 把二进制与其 `.so` 平铺到 bin_dir, 统一注入 `LD_LIBRARY_PATH=<bin_dir>`。
pub async fn ensure_bin_command(key: EnvKey) -> anyhow::Result<std::process::Command> {
    let bin = ensure_bin(key).await?;
    let mut cmd = std::process::Command::new(&bin);

    #[cfg(target_os = "linux")]
    {
        let bin_dir = bin.parent().unwrap_or_else(|| std::path::Path::new("."));
        let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let combined = if existing.is_empty() {
            bin_dir.to_string_lossy().into_owned()
        } else {
            format!("{}:{}", bin_dir.display(), existing)
        };
        tracing::info!(target: "env", "{} 注入 LD_LIBRARY_PATH={}", key.as_str(), combined);
        cmd.env("LD_LIBRARY_PATH", combined);
    }

    Ok(cmd)
}

/// 运行检查 (镜像 TS `runCheck`)。空 targets = 全量。
///
/// `desired` 为本次配置实际需要的后端映射 (如 `DemucsBurnBin` → "tch")，
/// 使 burn 系检查精确报告缺失的后端二进制。
pub async fn run_check(targets: &[EnvKey], desired: &HashMap<EnvKey, String>) -> Vec<CheckResult> {
    let selected: Vec<EnvKey> = if targets.is_empty() {
        EnvKey::all()
    } else {
        targets.to_vec()
    };
    let mut results = Vec::new();
    for key in &selected {
        results.push(key.check(desired.get(key).map(|s| s.as_str())).await);
    }
    results
}

/// 运行 ensure (镜像 TS `runEnsure`)。`desired` 仅 check 路径使用，这里接受以保持签名一致。
pub async fn run_ensure(targets: &[EnvKey], _desired: &HashMap<EnvKey, String>) -> Vec<CheckResult> {
    let selected: Vec<EnvKey> = if targets.is_empty() {
        EnvKey::all()
    } else {
        targets.to_vec()
    };
    let mut results = Vec::new();
    for key in &selected {
        match key.ensure().await {
            Some(r) => results.push(r),
            None => results.push(CheckResult {
                key: key.as_str().to_string(),
                status: CheckStatus::Skip,
                data: serde_json::json!({ "msg": "该项无安装动作" }),
                required: false,
            }),
        }
    }
    results
}

/// 从 data 提取简化 msg (优先 `msg`, 否则拼接关键字段)。
fn extract_msg(r: &CheckResult) -> String {
    if let Some(m) = r.data.get("msg").and_then(|v| v.as_str()) {
        if !m.is_empty() {
            return m.to_string();
        }
    }
    // 退化: 拼接 size / version / issues / found/total
    let mut parts = Vec::new();
    for k in ["size", "version", "issues", "missing", "gpu"] {
        if let Some(v) = r.data.get(k) {
            parts.push(v.to_string());
        }
    }
    if let (Some(f), Some(t)) = (r.data.get("found"), r.data.get("total")) {
        parts.push(format!("{f}/{t}"));
    }
    parts.join(" ").trim().to_string()
}

/// 格式化单行结果 (镜像 TS `formatResult`)。
pub fn format_result(r: &CheckResult) -> String {
    let prefix = match r.status {
        CheckStatus::Pass => "  ✓",
        CheckStatus::Warn => "  ⚠",
        CheckStatus::Fail | CheckStatus::Skip => "  ✗",
    };
    let msg = extract_msg(r);
    let first = format!("{prefix} {} — {}  ({:?})", r.key, msg, r.status);

    let mut extras: Vec<String> = Vec::new();
    if let Some(p) = r.data.get("path").and_then(|v| v.as_str()) {
        if !p.is_empty() {
            extras.push(format!("    path: {p}"));
        }
    }
    if let Some(m) = r.data.get("missing_bins").and_then(|v| v.as_str()) {
        if !m.is_empty() {
            extras.push(format!("    missing: {m}"));
        }
    }
    if let Some(s) = r.data.get("stale_bins").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            extras.push(format!("    stale: {s}"));
        }
    }
    if let Some(f) = r.data.get("fresh_bins").and_then(|v| v.as_str()) {
        if !f.is_empty() {
            extras.push(format!("    fresh: {f}"));
        }
    }
    if let Some(t) = r.data.get("latest_tag").and_then(|v| v.as_str()) {
        if !t.is_empty() {
            extras.push(format!("    upstream: {t} (有新版可更新)"));
        }
    }

    let desc = EnvKey::parse(&r.key)
        .map(|k| k.meta().zh)
        .unwrap_or("");
    let mut lines = vec![first];
    lines.extend(extras);
    if !desc.is_empty() {
        format!("{}\n  {}", lines.join("\n"), desc)
    } else {
        lines.join("\n")
    }
}
