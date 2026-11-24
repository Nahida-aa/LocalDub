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

    // --- LLM 步骤：只要有一个启用就检查其引用的 provider ---
    if steps.translate.enabled
        || steps.asr_fix.llm_fix.llm_fix
        || steps.sf_ocr_fix.llm_fix.llm_fix
        || steps.asr_ocr_fix.ocr_fix.llm_fix.llm_fix
    {
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

/// 收集步骤实际引用的 LLM provider：(名, base_url, api_key)，按 provider 名去重。
/// 供 env 的 openai 检查项探测「真正会被用到的端点」，而不是环境变量。
pub fn referenced_llm_providers(input: &Input) -> Vec<(String, String, Option<String>)> {
    let steps = &input.steps;
    let mut names: Vec<&str> = vec![
        steps.translate.provider.as_str(),
        steps.asr_fix.llm_fix.provider.as_str(),
        steps.sf_ocr_fix.llm_fix.provider.as_str(),
        steps.asr_ocr_fix.ocr_fix.llm_fix.provider.as_str(),
        steps.meta.provider.as_str(),
    ];
    names.dedup();
    let env_key = config_rs::env::openai_api_key();
    let mut seen_urls = std::collections::HashSet::new();
    names
        .into_iter()
        .filter_map(|name| {
            crate::input::resolve_llm_provider(&input.llm_providers, name).map(|p| {
                (
                    name.to_string(),
                    p.base_url.clone(),
                    p.api_key.clone().or_else(|| env_key.clone()),
                )
            })
        })
        // 唯一 provider 被多个名字引用时只探测一次（按 base_url 去重）
        .filter(|(_, url, _)| seen_urls.insert(url.clone()))
        .collect()
}

/// 运行检查 (镜像 TS `runCheck`)。空 targets = 全量。
///
/// `desired` 为本次配置实际需要的后端映射 (如 `DemucsBurnBin` → "tch")，
/// 使 burn 系检查精确报告缺失的后端二进制。`input` 供 openai 项解析实际 provider。
pub async fn run_check(
    targets: &[EnvKey],
    desired: &HashMap<EnvKey, String>,
    input: &Input,
) -> Vec<CheckResult> {
    let selected: Vec<EnvKey> = if targets.is_empty() {
        EnvKey::all()
    } else {
        targets.to_vec()
    };
    let providers = referenced_llm_providers(input);
    let mut results = Vec::new();
    for key in &selected {
        if *key == EnvKey::Openai {
            results.push(items::check_openai_providers(&providers).await);
        } else {
            results.push(key.check(desired.get(key).map(|s| s.as_str())).await);
        }
    }
    results
}

/// 运行 ensure (镜像 TS `runEnsure`)。ensure = 让环境就绪：**幂等**——
/// 先 check，已就绪就报告现状不重复下载；只在缺失/过期时才执行 ensure 动作；
/// 无自动动作的项（系统前置/外部服务）报告其 check 状态。
pub async fn run_ensure(targets: &[EnvKey], desired: &HashMap<EnvKey, String>) -> Vec<CheckResult> {
    let selected: Vec<EnvKey> = if targets.is_empty() {
        EnvKey::all()
    } else {
        targets.to_vec()
    };
    let mut results = Vec::new();
    for key in &selected {
        let check = key.check(desired.get(key).map(|s| s.as_str())).await;
        if check.status == CheckStatus::Pass {
            // 已就绪：直接报告，不重复 ensure（避免重下已下好的二进制）
            results.push(check);
            continue;
        }
        match key.ensure().await {
            Some(r) => results.push(r),
            // 无自动 ensure 动作的项：报告 check 结果，让用户知道缺什么。
            None => results.push(check),
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
