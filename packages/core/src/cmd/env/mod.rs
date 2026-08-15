//! 环境检测命令 (`cli env [check|ensure] [targets...]`)。
//!
//! 镜像 TS `packages/core/cmd/env/index.ts`: `runCheck` / `runEnsure` /
//! `formatResult` + `resolveTargets`。检查逻辑见 `items.rs`, 元信息见 `input.rs`。

pub mod input;
pub mod items;

use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::cmd::env::input::{env_names, zh_desc};
use crate::cmd::env::items::{all_checks, ensure_fns};
use crate::input::Input;
use crate::stages::tts::args::TtsDevice;

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

/// 全部环境项 key 集合 (镜像 TS `envList`)。
pub fn env_list() -> Vec<&'static str> {
    env_names()
}

/// 按 `input.jsonc` 的 stages 配置推断本次任务所需的环境项 (targets 为空时使用)。
///
/// 设计: 从「核心工具链 + 配置」基础集出发, 按各 stage 的 runtime/device 追加依赖。
/// 这是启发式推断 (非精确依赖图), 目标是给出「跟当前配置相关」的检查项, 避免每次扫全 31 项。
pub fn infer_targets(input: &Input) -> (Vec<String>, HashMap<String, String>) {
    use crate::stages::asr::args::{AsrDevice, Runtime as AsrRuntime};
    use crate::stages::separate::args::Device as SepDevice;
    use crate::stages::tts::args::{TtsDevice, TtsRuntime};
    use crate::tasks::args::SubtitleSource;

    let mut set: HashSet<String> = HashSet::new();
    let mut desired: HashMap<String, String> = HashMap::new();
    let add = |s: &str, set: &mut HashSet<String>| {
        set.insert(s.to_string());
    };

    // 基础: 任何任务都需要的工具链与配置
    for k in ["bun", "python", "uv", "ffmpeg", "git", "cmake", "dotenv"] {
        add(k, &mut set);
    }

    let stages = &input.stages;
    let subtitle_source = input
        .task
        .as_ref()
        .map(|t| t.subtitle_source)
        .unwrap_or(SubtitleSource::Asr);

    // --- sf_ocr / asr_ocr: OCR 阶段需要 ocr_cpp_bin (cmake 已在基础集) ---
    if subtitle_source == SubtitleSource::SfOcr {
        add("ocr_cpp_bin", &mut set);
    }
    // asr_ocr 阶段 (flatten 复用 SfOcrArgs, 无 enabled 开关) → 同样需要 ocr_cpp_bin
    // 仅当 subtitle_source 非纯 asr 时纳入 (asr 流程也会跑 asr_ocr 做校正)
    if subtitle_source != SubtitleSource::Asr {
        add("ocr_cpp_bin", &mut set);
    }

    // --- asr ---
    match stages.asr.runtime {
        AsrRuntime::Ggml => add("whisper_ggml", &mut set),
        // faster-whisper / pytorch 仍依赖 whisper 模型 (pth 形态, 这里用 ggml 占位检查)
        AsrRuntime::FasterWhisper | AsrRuntime::Pytorch => add("whisper_ggml", &mut set),
    }
    if stages.asr.vad {
        add("whisper_vad", &mut set);
    }
    if stages.asr.device == AsrDevice::Vulkan {
        add("whisper_bin", &mut set);
        add("vulkan", &mut set);
    }
    if stages.asr.device == AsrDevice::Cuda {
        add("cuda", &mut set);
    }
    if stages.asr.device == AsrDevice::Mps {
        add("cuda", &mut set); // mps 复用 apple 驱动检查 (无独立项)
    }

    // --- separate / demucs ---
    // asr.useSeparated 或 separate.always 表示需要人声分离
    if stages.asr.use_separated || stages.separate.always {
        add("demucs_pth", &mut set);
        add("demucs_burn_bin", &mut set);
        // 本次配置实际需要的 demucs 后端后缀 (bin = demucs-burn-{suffix})
        let suffix = demucs_backend_suffix(stages.separate.runtime, stages.separate.device);
        desired.insert("demucs_burn_bin".to_string(), suffix.to_string());
        match stages.separate.device {
            SepDevice::Vulkan => add("vulkan", &mut set),
            SepDevice::Cuda => add("cuda", &mut set),
            SepDevice::Webgpu => add("vulkan", &mut set), // webgpu 走 vulkan 驱动
            SepDevice::Cpu | SepDevice::Mps => {}
        }
    }

    // --- translate: 启用则依赖 openai 兼容 API ---
    if stages.translate.enabled {
        add("openai", &mut set);
    }

    // --- tts ---
    match stages.tts.runtime {
        TtsRuntime::Cloud => add("openai", &mut set), // 云端 TTS 走 OpenAI 兼容 API
        TtsRuntime::Ggml => {
            add("voxcpm2_onnx", &mut set);
            add("voxcpm_burn_bin", &mut set);
            desired.insert(
                "voxcpm_burn_bin".to_string(),
                voxcpm_backend_suffix(stages.tts.device).to_string(),
            );
        }
        TtsRuntime::VoxcpmTorchGradio => {
            add("voxcpm2_pth", &mut set);
            add("voxcpm_burn_bin", &mut set);
            desired.insert(
                "voxcpm_burn_bin".to_string(),
                voxcpm_backend_suffix(stages.tts.device).to_string(),
            );
        }
    }
    match stages.tts.device {
        TtsDevice::Webgpu => add("vulkan", &mut set),
        TtsDevice::Cuda => add("cuda", &mut set),
        TtsDevice::Rocm => add("rocm", &mut set),
        TtsDevice::Cpu | TtsDevice::Mps => {}
    }

    // 按 env_names 稳定顺序输出 (与 run_check 全量顺序一致)
    let targets = env_names()
        .into_iter()
        .filter(|n| set.contains(*n))
        .map(|s| s.to_string())
        .collect();
    (targets, desired)
}

/// separate 后端后缀: bin = demucs-burn-{suffix}。runtime 优先 (burn-tch→tch), 否则按 device 映射。
fn demucs_backend_suffix(
    runtime: crate::stages::separate::args::Runtime,
    device: crate::stages::separate::args::Device,
) -> &'static str {
    use crate::stages::separate::args::{Device as SepDevice, Runtime as SepRuntime};
    match runtime {
        SepRuntime::BurnTch => "tch",
        SepRuntime::Burn => match device {
            SepDevice::Cpu | SepDevice::Mps => "cpu",
            SepDevice::Cuda => "cuda",
            SepDevice::Vulkan => "vulkan",
            SepDevice::Webgpu => "wgpu",
        },
    }
}

/// tts 后端后缀: bin = voxcpm-burn-{suffix} (按 device 映射)。
fn voxcpm_backend_suffix(device: TtsDevice) -> &'static str {
    match device {
        TtsDevice::Cpu | TtsDevice::Mps => "cpu",
        TtsDevice::Cuda => "cuda",
        TtsDevice::Rocm => "rocm",
        TtsDevice::Webgpu => "wgpu",
    }
}

/// 解析目标: 空 → 全部; 过滤到 all_checks 中存在的 key; 过滤后空 → 全部。
fn resolve_targets(targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return env_names().iter().map(|s| s.to_string()).collect();
    }
    let checks = all_checks();
    let valid: Vec<String> = targets
        .iter()
        .filter(|t| checks.contains_key(t.as_str()))
        .cloned()
        .collect();
    if valid.is_empty() {
        return env_names().iter().map(|s| s.to_string()).collect();
    }
    valid
}

/// 运行检查 (镜像 TS `runCheck`)。
///
/// `desired` 为「本次配置实际需要的环境项后端」映射 (如 `demucs_burn_bin` → "tch"),
/// 用于让 burn 系检查精确报告缺失的后端二进制, 而非笼统列出全部变体。
pub fn run_check(targets: &[String], desired: &HashMap<String, String>) -> Vec<CheckResult> {
    let selected = resolve_targets(targets);
    let checks = all_checks();
    let mut results = Vec::new();
    for key in &selected {
        // burn 系二进制: 传入本次配置所需的后端后缀, 精确报告
        if key == "demucs_burn_bin" {
            results.push(crate::cmd::env::items::check_demucs_burn_bin(
                desired.get("demucs_burn_bin").map(|s| s.as_str()),
            ));
            continue;
        }
        if key == "voxcpm_burn_bin" {
            results.push(crate::cmd::env::items::check_voxcpm_burn_bin(
                desired.get("voxcpm_burn_bin").map(|s| s.as_str()),
            ));
            continue;
        }
        match checks.get(key.as_str()) {
            Some(f) => {
                // 单 check 内部已吞异常式处理; 这里再包一层防御
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(
                    |_| CheckResult {
                        key: key.clone(),
                        status: CheckStatus::Fail,
                        data: serde_json::json!({ "msg": "检查过程 panic" }),
                        required: false,
                    },
                );
                results.push(r);
            }
            None => results.push(CheckResult {
                key: key.clone(),
                status: CheckStatus::Skip,
                data: serde_json::json!({}),
                required: false,
            }),
        }
    }
    results
}

/// 运行 ensure (镜像 TS `runEnsure`)。`desired` 目前仅 check 路径使用, 这里接受以保持签名一致。
pub fn run_ensure(targets: &[String], _desired: &HashMap<String, String>) -> Vec<CheckResult> {
    let selected = resolve_targets(targets);
    let fns = ensure_fns();
    let mut results = Vec::new();
    for key in &selected {
        match fns.get(key.as_str()) {
            Some(f) => {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(
                    |_| CheckResult {
                        key: key.clone(),
                        status: CheckStatus::Fail,
                        data: serde_json::json!({ "msg": "ensure 过程 panic" }),
                        required: false,
                    },
                );
                results.push(r);
            }
            None => results.push(CheckResult {
                key: key.clone(),
                status: CheckStatus::Skip,
                data: serde_json::json!({}),
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

    let desc = zh_desc(&r.key);
    let mut lines = vec![first];
    lines.extend(extras);
    if !desc.is_empty() {
        format!("{}\n  {}", lines.join("\n"), desc)
    } else {
        lines.join("\n")
    }
}
