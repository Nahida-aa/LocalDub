//! 环境检测的具体检查项 (镜像 TS `packages/core/cmd/env/items.ts`)。
//!
//! 设计抉择 (见 plan):
//! - i18n: TS 用 `@repo/shared/i18n` 的 `t(key)`; Rust 无框架, 各 check 在 `data`
//!   里给出简化 `msg` 字段, `format_result` 直接打印 + 中文描述 (EnvKey::meta)。
//! - ensure 语义: OpenAI 兼容服务由 provider 配置外部管理, 无法在本工具内
//!   安装/启动——`ensure_openai` 只报告信息（等同 check），不再起 ollama。
//! - try_exec 超时: TS 用 `spawnSync` 10s 超时; Rust 用 spawn + try_wait 轮询 +
//!   超时 kill (`TRY_EXEC_TIMEOUT`), 超时视为 ok=false (对齐 TS)。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono;
use serde_json::json;

use config_rs::env::{openai_api_key, openai_base_url};
use config_rs::path::models::{bin_dir, demucs_model_dir, voxcpm_model_dir, whisper_model_dir};
use config_rs::root::repo_root;

use crate::cmd::env::input::EnvKey;
use crate::cmd::env::{CheckResult, CheckStatus};

// ---------------------------------------------------------------------------
// 本地 helper
// ---------------------------------------------------------------------------

/// 镜像 TS `tryExec` (spawnSync timeout:10s): 同步执行命令, 超时 kill 视为 ok=false。
fn try_exec(cmd: &str, args: &[&str], cwd: Option<&Path>) -> (bool, String, String) {
    crate::utils::process::run_cmd(cmd, args.iter().copied(), cwd, TRY_EXEC_TIMEOUT, true)
}

/// `tryExec` 超时 (镜像 TS spawnSync timeout:10s)。
const TRY_EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn fmt_size(bytes: u64) -> String {
    if bytes > 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else if bytes > 1_000_000 {
        format!("{} MB", bytes / 1_000_000)
    } else if bytes > 1_000 {
        format!("{} KB", bytes / 1_000)
    } else {
        format!("{bytes} B")
    }
}

/// 模型大小检查 (镜像 TS `checkModel`)。min_mb 支持小数 (如 silero vad 0.5MB)。
fn check_model(path: &Path, key: &str, min_mb: f64) -> CheckResult {
    let path_str = path.display().to_string();
    match file_size(path) {
        None => CheckResult {
            key: key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "path": path_str, "msg": format!("模型不存在: {}", path_str) }),
            required: false,
        },
        Some(size) => {
            let mb = size as f64 / 1e6;
            if mb < min_mb {
                CheckResult {
                    key: key.to_string(),
                    status: CheckStatus::Warn,
                    data: json!({ "path": path_str, "size": fmt_size(size), "msg": format!("模型偏小: {}", fmt_size(size)) }),
                    required: false,
                }
            } else {
                CheckResult {
                    key: key.to_string(),
                    status: CheckStatus::Pass,
                    data: json!({ "path": path_str, "size": fmt_size(size), "msg": format!("大小 {}", fmt_size(size)) }),
                    required: false,
                }
            }
        }
    }
}

/// `.venv` 下的 python 可执行文件 (镜像 TS `pythonBin`)。
fn python_bin() -> PathBuf {
    let base = repo_root().join(".venv");
    if cfg!(windows) {
        base.join("Scripts").join("python.exe")
    } else {
        base.join("bin").join("python")
    }
}

// ---------------------------------------------------------------------------
// 工具: 版本/路径辅助
// ---------------------------------------------------------------------------

/// 首个匹配的语义化版本号 (x.y.z)。
fn first_version(s: &str) -> String {
    // 简单扫描: 找 `数字.数字.数字`
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i].is_ascii_digit() {
            // 尝试解析后续 "d+.d+.d+"
            if let Some(end) = s[i..].find(|c: char| !(c.is_ascii_digit() || c == '.')) {
                let cand = &s[i..i + end];
                if cand.matches('.').count() == 2 {
                    return cand.to_string();
                }
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// 基础工具链检查
// ---------------------------------------------------------------------------

pub fn check_bun() -> CheckResult {
    let (ok, out, _) = try_exec("bun", &["--version"], None);
    if !ok {
        return CheckResult {
            key: "bun".into(),
            status: CheckStatus::Fail,
            data: json!({}),
            required: true,
        };
    }
    CheckResult {
        key: "bun".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": out, "msg": format!("bun {}", out) }),
        required: true,
    }
}

pub fn check_python() -> CheckResult {
    let py = python_bin();
    if !py.exists() {
        return CheckResult {
            key: "python".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "未找到 .venv 下的 python" }),
            required: true,
        };
    }
    let (ok, out, _) = try_exec(py.to_str().unwrap(), &["--version"], None);
    // python --version 输出到 stderr, 故取 stdout+stderr 拼接
    let ver = if ok {
        let v = format!("{out}");
        // 提取版本号
        let m = v
            .split_whitespace()
            .find(|w| {
                w.chars().filter(|c| c == &'.').count() == 2
                    && w.chars().any(|c| c.is_ascii_digit())
            })
            .unwrap_or(&v)
            .to_string();
        m
    } else {
        String::new()
    };
    CheckResult {
        key: "python".into(),
        status: if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        data: json!({ "version": ver, "path": py.display().to_string(), "msg": format!("python {}", ver) }),
        required: true,
    }
}

pub fn check_uv() -> CheckResult {
    let (ok, out, _) = try_exec("uv", &["--version"], None);
    if !ok {
        return CheckResult {
            key: "uv".into(),
            status: CheckStatus::Fail,
            data: json!({}),
            required: true,
        };
    }
    let version = out.split_whitespace().nth(1).unwrap_or(&out).to_string();
    let (py_ok, py_out, _) = try_exec("uv", &["python", "find"], None);
    let python_path = if py_ok { py_out } else { String::new() };
    CheckResult {
        key: "uv".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": version, "pythonPath": python_path, "msg": format!("uv {}", version) }),
        required: true,
    }
}

pub fn check_ffmpeg() -> CheckResult {
    let bin = std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());
    let (ok, out, _) = try_exec(&bin, &["-version"], None);
    if !ok {
        return CheckResult {
            key: "ffmpeg".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "ffmpeg 不可用" }),
            required: true,
        };
    }
    let ver = out
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("ffmpeg version "))
        .map(|s| s.split_whitespace().next().unwrap_or(s).to_string())
        .unwrap_or_default();
    let has_x264 = out.contains("libx264");
    let has_mp3 = out.contains("libmp3lame");
    let codecs = {
        let mut v = Vec::new();
        if has_x264 {
            v.push("libx264");
        }
        if has_mp3 {
            v.push("libmp3lame");
        }
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    let data =
        json!({ "version": ver, "codecs": codecs, "msg": format!("ffmpeg {} ({})", ver, codecs) });
    // 缺关键 codec 降级为 warn
    if !has_x264 || !has_mp3 {
        return CheckResult {
            key: "ffmpeg".into(),
            status: CheckStatus::Warn,
            data,
            required: true,
        };
    }
    CheckResult {
        key: "ffmpeg".into(),
        status: CheckStatus::Pass,
        data,
        required: true,
    }
}

pub fn check_cargo() -> CheckResult {
    let (ok, out, _) = try_exec("cargo", &["--version"], None);
    if !ok {
        return CheckResult {
            key: "cargo".into(),
            status: CheckStatus::Fail,
            data: json!({}),
            required: false,
        };
    }
    let ver = first_version(&out);
    CheckResult {
        key: "cargo".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": ver, "msg": format!("cargo {}", ver) }),
        required: false,
    }
}

pub fn check_vulkan() -> CheckResult {
    let (ok, out, _) = try_exec("vulkaninfo", &["--summary"], None);
    if !ok {
        return CheckResult {
            key: "vulkan".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "vulkaninfo 不可用" }),
            required: false,
        };
    }
    let gpu = out
        .lines()
        .find(|l| l.contains("GPU") || l.contains("deviceName"))
        .and_then(|l| l.split(':').next_back())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    CheckResult {
        key: "vulkan".into(),
        status: CheckStatus::Pass,
        data: json!({ "gpu": gpu, "msg": if gpu.is_empty() { "vulkan 可用".to_string() } else { format!("GPU: {gpu}") } }),
        required: false,
    }
}

pub fn check_rocm() -> CheckResult {
    let (ok, _, _) = try_exec("rocm-smi", &[], None);
    if !ok {
        return CheckResult {
            key: "rocm".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "rocm-smi 不可用" }),
            required: false,
        };
    }
    CheckResult {
        key: "rocm".into(),
        status: CheckStatus::Pass,
        data: json!({ "msg": "rocm 可用" }),
        required: false,
    }
}

pub fn check_cuda() -> CheckResult {
    let (ok, out, _) = try_exec("nvidia-smi", &[], None);
    if !ok {
        return CheckResult {
            key: "cuda".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "nvidia-smi 不可用" }),
            required: false,
        };
    }
    let ver = out
        .lines()
        .find_map(|l| l.split("CUDA Version:").nth(1))
        .map(|s| s.trim().split_whitespace().next().unwrap_or("").to_string())
        .unwrap_or_default();
    CheckResult {
        key: "cuda".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": ver, "msg": format!("CUDA {}", ver) }),
        required: false,
    }
}

// ---------------------------------------------------------------------------
// 子模块检查
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 编译产物检查
// ---------------------------------------------------------------------------

/// whisper-vulkan 由 vox-lab 预编译发布 (ReleaseBinSpec), 见 `WHISPER_VULKAN`。
pub async fn check_whisper_bin() -> CheckResult {
    check_release_bin(&WHISPER_VULKAN).await
}

/// 检查 burn 系二进制。
pub fn check_demucs_burn_bin(required: Option<&str>) -> CheckResult {
    // demucs-burn 已迁至 vox-lab, 仅 tch/wgpu 有 release 资产 (经 demucs_burn_{tch,wgpu}_bin
    // 检查)。cpu/cuda/vulkan/rocm 等后端暂无发布资产, 源码构建路径也已移除。
    let _ = required;
    CheckResult {
        key: "demucs_burn_bin".into(),
        status: CheckStatus::Fail,
        data: json!({ "msg": "该 demucs-burn 后端无发布资产, 请改用 separate.runtime=wgpu / tch / burn-tch" }),
        required: false,
    }
}

pub fn check_cmake() -> CheckResult {
    let (ok, out, _) = try_exec("cmake", &["--version"], None);
    if !ok {
        return CheckResult {
            key: "cmake".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "cmake 不可用" }),
            required: false,
        };
    }
    let ver = first_version(&out);
    CheckResult {
        key: "cmake".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": ver, "msg": format!("cmake {}", ver) }),
        required: false,
    }
}

pub fn check_git() -> CheckResult {
    let (ok, out, _) = try_exec("git", &["--version"], None);
    if !ok {
        return CheckResult {
            key: "git".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "git 不可用" }),
            required: false,
        };
    }
    let ver = first_version(&out);
    CheckResult {
        key: "git".into(),
        status: CheckStatus::Pass,
        data: json!({ "version": ver, "msg": format!("git {}", ver) }),
        required: false,
    }
}

/// yt-dlp: `YTDLP_BIN` 覆盖路径，否则按 `which("yt-dlp")`（与 `import/util.rs` 同一解析）。
/// 原先该项只在 ENV_ENTRIES 里、无 check 实现 → 静默 Skip；枚举化后给真实检查。
pub fn check_ytdlp() -> CheckResult {
    let bin = std::env::var("YTDLP_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| which("yt-dlp"));
    let Some(bin) = bin else {
        return CheckResult {
            key: "yt_dlp".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "yt-dlp 未找到 (可设 YTDLP_BIN 指定路径)" }),
            required: false,
        };
    };
    let (ok, out, _) = try_exec(&bin, &["--version"], None);
    if !ok {
        return CheckResult {
            key: "yt_dlp".into(),
            status: CheckStatus::Warn,
            data: json!({ "path": bin, "msg": "yt-dlp 存在但无法执行 --version" }),
            required: false,
        };
    }
    let ver = first_version(&out);
    CheckResult {
        key: "yt_dlp".into(),
        status: CheckStatus::Pass,
        data: json!({ "path": bin, "version": ver, "msg": format!("yt-dlp {ver}") }),
        required: false,
    }
}

// ---------------------------------------------------------------------------
// 模型文件检查
// ---------------------------------------------------------------------------

pub fn check_whisper_ggml() -> CheckResult {
    check_model(
        &whisper_model_dir().join("ggml-large-v3-turbo.bin"),
        "whisper_ggml",
        1500.0,
    )
}

pub fn check_whisper_vad() -> CheckResult {
    check_model(
        &whisper_model_dir().join("ggml-silero-v6.2.0.bin"),
        "whisper_vad",
        0.5, // 镜像 TS: silero vad 最小 0.5MB
    )
}

/// demucs 权重检查: separate 的 `model` 可选 htdemucs / htdemucs_6s / htdemucs_ft,
/// 故只要**任一**模型已缓存即通过; 全缺才 Fail。大小阈值按文件各自给 (htdemucs 与
/// htdemucs_6s 约 84MB, htdemucs_ft 约 333MB), 共用 300MB 会把小模型误判为偏小。
pub fn check_demucs_pth() -> CheckResult {
    const MODELS: &[(&str, f64)] = &[
        ("htdemucs_ft.safetensors", 300.0),
        ("htdemucs.safetensors", 70.0),
        ("htdemucs_6s.safetensors", 70.0),
    ];
    let dir = demucs_model_dir();

    let mut first: Option<CheckResult> = None;
    let mut found = Vec::new();
    for (file, min_mb) in MODELS {
        let r = check_model(&dir.join(file), "demucs_pth", *min_mb);
        if r.status != CheckStatus::Fail {
            found.push(file.trim_end_matches(".safetensors").to_string());
            if first.is_none() {
                first = Some(r);
            }
        }
    }
    match first {
        Some(mut r) => {
            r.data["models"] = serde_json::json!(found);
            r
        }
        None => {
            let expected: Vec<String> = MODELS.iter().map(|(f, _)| f.to_string()).collect();
            CheckResult {
                key: "demucs_pth".into(),
                status: CheckStatus::Fail,
                data: serde_json::json!({
                    "path": dir.display().to_string(),
                    "msg": format!("未找到任何 demucs 模型 (期望其一: {})", expected.join(", ")),
                }),
                required: false,
            }
        }
    }
}

pub fn check_voxcpm2_onnx() -> CheckResult {
    let files = [
        "voxcpm2_prefill.onnx",
        "voxcpm2_prefill.onnx.data",
        "voxcpm2_decode_step.onnx",
        "voxcpm2_decode_step.onnx.data",
        "audio_vae_decoder.onnx",
        "audio_vae_decoder.onnx.data",
        "audio_vae_encoder.onnx",
        "audio_vae_encoder.onnx.data",
    ];
    let mut missing = Vec::new();
    for f in files {
        if !voxcpm_model_dir().join(f).exists() {
            missing.push(f.to_string());
        }
    }
    let found = files.len() - missing.len();
    let status = if missing.is_empty() {
        CheckStatus::Pass
    } else if missing.len() == files.len() {
        CheckStatus::Fail
    } else {
        CheckStatus::Warn
    };
    CheckResult {
        key: "voxcpm2_onnx".into(),
        status,
        data: json!({ "found": found, "total": files.len(), "missing": missing.join(", "), "msg": if missing.is_empty() { "voxcpm2 onnx 齐全".to_string() } else { format!("缺失 {} ({}/{})", missing.join(", "), found, files.len()) } }),
        required: false,
    }
}

pub fn check_voxcpm2_pth() -> CheckResult {
    let model = voxcpm_model_dir().join("model.safetensors");
    let vae = voxcpm_model_dir().join("audiovae.pth");
    let model_size = file_size(&model);
    let vae_size = file_size(&vae);
    match (model_size, vae_size) {
        (None, _) | (_, None) => {
            let mut missing = Vec::new();
            if model_size.is_none() {
                missing.push("model.safetensors");
            }
            if vae_size.is_none() {
                missing.push("audiovae.pth");
            }
            CheckResult {
                key: "voxcpm2_pth".into(),
                status: CheckStatus::Fail,
                data: json!({ "missing": missing.join(", "), "msg": format!("缺失 {}", missing.join(", ")) }),
                required: false,
            }
        }
        (Some(ms), Some(vs)) => CheckResult {
            key: "voxcpm2_pth".into(),
            status: CheckStatus::Pass,
            data: json!({ "modelSize": fmt_size(ms), "vaeSize": fmt_size(vs), "msg": format!("model {}, vae {}", fmt_size(ms), fmt_size(vs)) }),
            required: false,
        },
    }
}

// ---------------------------------------------------------------------------
// dotenv
// ---------------------------------------------------------------------------

pub fn check_dotenv() -> CheckResult {
    let env_path = repo_root().join(".env");
    if !env_path.exists() {
        return CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": ".env 不存在" }),
            required: false,
        };
    }
    let content = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut issues = Vec::new();
    if !content.contains("DEVICE=") {
        issues.push("DEVICE not set");
    }
    if !content.contains("OPENAI_API_KEY=") {
        issues.push("OPENAI_API_KEY not set");
    }
    if !issues.is_empty() {
        return CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Warn,
            data: json!({ "issues": issues.join(", "), "msg": issues.join(", ") }),
            required: false,
        };
    }
    CheckResult {
        key: "dotenv".into(),
        status: CheckStatus::Pass,
        data: json!({ "msg": ".env 配置完整" }),
        required: false,
    }
}

// ---------------------------------------------------------------------------
// OpenAI (兼容 API) 检查 + ensure (ollama serve)
// ---------------------------------------------------------------------------

pub async fn check_openai() -> CheckResult {
    // 保留：无 input 上下文的调用方（如 CLI `--targets openai` 但没配 provider）探测环境变量。
    check_openai_endpoint("environment", &openai_base_url(), openai_api_key().as_deref()).await
}

/// 检查步骤实际引用的 provider：对每个不同 base_url 探测 /models，任一失败即 Fail。
/// `providers` 为 (provider 名, base_url, api_key) 列表，由 mod.rs 从 input 收集。
pub async fn check_openai_providers(
    providers: &[(String, String, Option<String>)],
) -> CheckResult {
    if providers.is_empty() {
        return check_openai().await; // 没有 provider 引用 → 回退环境变量探测
    }
    let mut results = Vec::new();
    let mut failures = Vec::new();
    for (name, base_url, key) in providers {
        let r = check_openai_endpoint(name, base_url, key.as_deref()).await;
        if r.status != CheckStatus::Pass {
            // 带上每个 provider 自己的失败原因，不然只报「不可达」无法区分
            // 无 key / HTTP 401 / 连接失败。
            let reason = r
                .data
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知原因");
            failures.push(format!("{name} ({base_url}): {reason}"));
        }
        results.push(r);
    }
    if failures.is_empty() {
        return CheckResult {
            key: "openai".into(),
            status: CheckStatus::Pass,
            data: json!({ "msg": format!("{} 个 provider 可达", results.len()) }),
            required: false,
        };
    }
    CheckResult {
        key: "openai".into(),
        status: CheckStatus::Fail,
        data: json!({ "issues": failures.join(", "), "msg": format!("provider 不可达: {}", failures.join(", ")) }),
        required: false,
    }
}

async fn check_openai_endpoint(name: &str, base_url: &str, api_key: Option<&str>) -> CheckResult {
    if (base_url.is_empty() || api_key.is_none())
        && !base_url.contains("localhost")
        && !base_url.contains("127.0.0.1")
    {
        return CheckResult {
            key: "openai".into(),
            status: CheckStatus::Fail,
            data: json!({ "issues": "不存在", "msg": format!("provider {name}: base_url / api_key 未配置") }),
            required: false,
        };
    }

    let is_local = base_url.contains("localhost") || base_url.contains("127.0.0.1");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                key: "openai".into(),
                status: CheckStatus::Fail,
                data: json!({ "issues": e.to_string(), "msg": "无法构造 HTTP 客户端" }),
                required: false,
            };
        }
    };

    let req = if is_local {
        client.get(format!("{base_url}/models"))
    } else {
        client
            .get(format!("{base_url}/models"))
            .bearer_auth(api_key.unwrap_or_default())
    };

    match req.send().await {
        Ok(res) if res.status().is_success() => {
            let models = res
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|j| j.get("data").and_then(|d| d.as_array()).map(|a| a.len()))
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            CheckResult {
                key: "openai".into(),
                status: CheckStatus::Pass,
                data: json!({ "provider": name, "baseUrl": base_url, "models": format!("{models} models"), "msg": format!("{name} 可达 ({} models)", models) }),
                required: false,
            }
        }
        Ok(res) => CheckResult {
            key: "openai".into(),
            status: CheckStatus::Warn,
            data: json!({ "issues": format!("HTTP {}", res.status()), "msg": format!("{name}: HTTP {}", res.status()) }),
            required: false,
        },
        Err(e) => CheckResult {
            key: "openai".into(),
            status: CheckStatus::Fail,
            data: json!({ "issues": e.to_string(), "msg": format!("{name} 连接失败: {e}") }),
            required: false,
        },
    }
}

/// ensure 语义（现在）：OpenAI 兼容服务由 provider 配置外部管理，本工具无法
/// （也不应）代装/代启。这里只**报告**当前 provider 情况，不做任何安装动作。
/// 真正的可达性探测在 `check_openai_providers` / `check_openai`。
async fn ensure_openai() -> CheckResult {
    check_openai().await
}

/// 在 PATH 中查找可执行文件 (镜像 `Bun.which`)。
fn which(name: &str) -> Option<String> {
    if let Some(p) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&p) {
            let candidate = if cfg!(windows) {
                dir.join(format!("{name}.exe"))
            } else {
                dir.join(name)
            };
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// vision-lab release 二进制 (subtitle-finder / subtitle-ocr / subtitle-ocr-post):
// 从 vision-lab GitHub Release 下载, 校验 sha256 后写版本戳 (版本戳管理防重下)。
//
// 资产命名规范: Linux `<bin>-<target-triple>` (单文件), Windows `<bin>-<target-triple>.zip`
// (exe + 运行时 dll 平铺), 同一 release 内多平台资产共存; 各平台 sha256 独立记录,
// 未发布的平台为 None (check/ensure 报"待发布"而非 404)。Windows zip 经过 sha256 校验后
// 解压到 bin_dir, 本地可执行文件名为 `<bin>.exe`。
//
// 模型目录默认相对仓库根: vision-lab CLI 用 current_exe_repo_root() 上溯两级解析
// (target/release 深度), data/bin 与其同深度, 故落位 data/bin 后能正确解析到
// 仓库根 data/models/rapidocr。
//
// ⚠️ vision-lab 09dc6ac 起包名/bin 名统一 `-` → `_` (subtitle_ocr / subtitle_finder /
// subtitle_ocr_post)。本文件仍按下划线前的旧 bin/资产名下载 —— 因为下面 pin 的是
// 历史 release tag (subtitle-ocr-v0.1.1 等), 其资产名不可变。下次 bump 新 release
// tag 时, 需同步改各 spec 的 bin/linux_asset/windows_asset (steps 侧经 ensure_bin
// 派生二进制路径, 会自动跟随; items.rs/input.rs 里的日志与提示文案需手改)。
// ---------------------------------------------------------------------------

/// 单个 release 二进制的下载描述 (按平台区分资产 + sha256)。
struct ReleaseBinSpec {
    /// 环境项 key (check/ensure 调度键)。
    key: &'static str,
    /// 二进制名 (日志/错误展示用, 如 "subtitle-ocr" 或 "demucs-burn-wgpu")。
    bin: &'static str,
    /// 托管 GitHub repo (owner/repo)。vision-lab 管 OCR 家族, vox-lab 管 demucs。
    repo: &'static str,
    /// GitHub Release tag。
    tag: &'static str,
    /// 资产是否为 zip (两平台统一, 下载后解压平铺到 bin_dir)。
    /// false: Linux 单文件资产 (资产名即本地文件名), Windows zip。
    /// true: Linux/Windows 均为 zip, 解压出 `<bin>`/`<bin>.exe` + 运行时 dll/.so 平铺。
    zip: bool,
    /// Linux x86_64 (+avx2 基线) 资产名 + sha256。
    linux_asset: &'static str,
    linux_sha256: &'static str,
    /// Windows x86_64 资产名 (含 .exe) + sha256; 未发布为 None。
    windows_asset: Option<&'static str>,
    windows_sha256: Option<&'static str>,
    /// 版本戳文件名 (与二进制同目录)。
    stamp: &'static str,
    /// true: 该二进制独占 `bin_dir/<bin>/` 子目录，zip 解压/版本戳/LD 都落在其中，
    ///       不与其它二进制共享 so（自包含，避免同名 so 冲突）。
    /// false: 历史平铺布局，所有二进制与 so 混在 `bin_dir/`。
    dir_layout: bool,
}

/// Linux 资产为静态链接版 zip (OpenCV 5.0 静态编进二进制, ffmpeg .so 随包平铺),
/// 由 vision-lab 的 release-subtitle-finder-static-linux.yml 构建。运行时由
/// `sf_ocr_pre` 注入 LD_LIBRARY_PATH=<bin dir> 找到随包 .so。
const SUBTITLE_FINDER: ReleaseBinSpec = ReleaseBinSpec {
    key: "subtitle_finder_bin",
    bin: "subtitle-finder",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-finder-v0.1.1",
    zip: true,
    linux_asset: "subtitle-finder-x86_64-unknown-linux-gnu.zip",
    // 由 release-subtitle-finder-static-linux.yml (run 44912342673) 实测产出
    linux_sha256: "0faa4f3475773be3cd665051046f441648b99c78b6bc047f780ccf2ac72c58c9",
    windows_asset: Some("subtitle-finder-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("9dece5e3cd2a9d72716d5fab5ecec256406b5113e38a479f55c76c43635396cc"),
    stamp: ".subtitle_finder.version.json",
    dir_layout: true, // 130 个 so 自包含，独占 data/bin/subtitle-finder/
};

const SUBTITLE_OCR: ReleaseBinSpec = ReleaseBinSpec {
    key: "subtitle_ocr_bin",
    bin: "subtitle-ocr",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-ocr-v0.1.1",
    // 静态链 OpenCV 5.0 的新资产（消除系统 OpenCV 5.0/5.1 soname 不匹配）；
    // zip 内单文件 subtitle-ocr，解压平铺到 bin_dir。旧裸二进制资产仍并存于 release。
    zip: true,
    linux_asset: "subtitle-ocr-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "65363ea934a5dcc664bb190b61564f44b26624e0451cca76682f46c657f4f591",
    windows_asset: Some("subtitle-ocr-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("7dfec92f6089b6213c0be9e69ee9c589a5730c5acb6dc6b33da96dffbf0389e7"),
    stamp: ".subtitle_ocr.version.json",
    dir_layout: false,
};

const SUBTITLE_OCR_POST: ReleaseBinSpec = ReleaseBinSpec {
    key: "subtitle_ocr_post_bin",
    bin: "subtitle-ocr-post",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-ocr-post-v0.1.3",
    zip: false,
    linux_asset: "subtitle-ocr-post-x86_64-unknown-linux-gnu",
    linux_sha256: "2a4bddbb573410e1f10d60490ca71734e3b58b57e963fb7a641f97923f680939",
    windows_asset: Some("subtitle-ocr-post-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("f075f1655c4df0088ab49eaaecf78ff005fbfc2c5a54cd6f5527200d679df89d"),
    stamp: ".subtitle_ocr_post.version.json",
    dir_layout: false,
};

const DEMUCS_BURN_TCH: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_burn_tch_bin",
    bin: "demucs-burn-tch",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-burn-v0.1.0",
    zip: true,
    linux_asset: "demucs-burn-tch-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "3cd530a17dedbff53c4ee86befd2567621e3d3a335b93f2cf313ebe414a79308",
    windows_asset: Some("demucs-burn-tch-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("c974ccd160dd330c38f1f1776df353624d8fc913b89b9619c745b14607836f45"),
    stamp: ".demucs_burn_tch.version.json",
    dir_layout: false,
};

const DEMUCS_BURN_WGPU: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_burn_wgpu_bin",
    bin: "demucs-burn-wgpu",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-burn-v0.1.0",
    zip: true,
    linux_asset: "demucs-burn-wgpu-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "f55bf5a80ae6fe9155df68eb652c70bceab422045eb3d3dbca703f9ec5c310e8",
    windows_asset: Some("demucs-burn-wgpu-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("e2aa3855f07f6cbfda8507db3eeecf5d8fc2151af13fa2908a6ae005207b3592"),
    stamp: ".demucs_burn_wgpu.version.json",
    dir_layout: false,
};

/// demucs-wgpu: 自研 WGSL/Vulkan kernel 路径 (不依赖 libtorch / ROCm 库)。
///
/// 单 exe, 经系统 Vulkan 运行; zip 内即 bin dir 内容。
const DEMUCS_WGPU: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_wgpu_bin",
    bin: "demucs-wgpu",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-wgpu-v0.1.0",
    zip: true,
    linux_asset: "demucs-wgpu-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "5ffce0de774f9c50a3e46d28c701c9599f36946f9d4d1bad758d847b3061c4da",
    windows_asset: Some("demucs-wgpu-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("10000663d73f700c13ac7cef58c7e80e25e717954490ca2edba6853210986be4"),
    stamp: ".demucs_wgpu.version.json",
    dir_layout: false,
};

/// demucs-tch: 独立 tch-rs 运行时 (不依赖 burn), 按设备分 cpu / cuda 变体。
///
/// **尚未发布** (`demucs-tch-v0.1.0` 在 vox-lab 侧还没有 tag/资产), 故这里只登记
/// tag 与资产名, sha256 留空——`ensure_demucs_tch_bin` 会在发布前明确报「未发布」,
/// 而不是给出一个假的校验值。发布后回填 sha256 即可。
const DEMUCS_TCH_CPU: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_tch_cpu_bin",
    bin: "demucs-tch-cpu",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-tch-v0.1.0",
    zip: true,
    linux_asset: "demucs-tch-cpu-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "",
    windows_asset: Some("demucs-tch-cpu-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some(""),
    stamp: ".demucs_tch_cpu.version.json",
    dir_layout: false,
};

const DEMUCS_TCH_CUDA: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_tch_cuda_bin",
    bin: "demucs-tch-cuda",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-tch-v0.1.0",
    zip: true,
    linux_asset: "demucs-tch-cuda-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "",
    windows_asset: Some("demucs-tch-cuda-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some(""),
    stamp: ".demucs_tch_cuda.version.json",
    dir_layout: false,
};

/// whisper-vulkan (vox-lab 预编译 whisper.cpp, GGML_VULKAN)。
///
/// whisper.cpp 官方 Release 只发 CPU/cublas 构建, 无 Vulkan 构建, 由 vox-lab
/// `release-whisper-linux.yml` 自建 CI 预编译 (GGML_NATIVE=OFF 便携基线)。
/// zip 内平铺 whisper-vulkan + libwhisper/libggml/libparakeet .so (运行时 LD_LIBRARY_PATH=bin_dir)。
const WHISPER_VULKAN: ReleaseBinSpec = ReleaseBinSpec {
    key: "whisper_bin",
    bin: "whisper-vulkan",
    repo: "Nahida-aa/vox-lab",
    tag: "whisper-cpp-v0.1.0",
    zip: true,
    linux_asset: "whisper-vulkan-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "1bdb026180cd21eaf31d84521ec64e92b32af261c32c87d2ca518337c2563c0c",
    windows_asset: Some("whisper-vulkan-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("1754cffc2132d7bbf303c65a27e7b01b578e76714afef97099e3d411d1cf9322"),
    stamp: ".whisper_vulkan.version.json",
    dir_layout: false,
};

/// 当前平台的资产 (asset 名, sha256); 平台未发布返回 None。
fn current_platform_asset(spec: &ReleaseBinSpec) -> Option<(&'static str, &'static str)> {
    if cfg!(windows) {
        match (spec.windows_asset, spec.windows_sha256) {
            (Some(a), Some(s)) => Some((a, s)),
            _ => None,
        }
    } else {
        Some((spec.linux_asset, spec.linux_sha256))
    }
}

/// 当前平台标签 (用于未发布提示)。
fn platform_label() -> &'static str {
    if cfg!(windows) {
        "Windows x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "linux x86_64"
    }
}

/// 二进制的落点根目录: dir_layout spec 独占 `bin_dir/<bin>/`, 其余平铺在 `bin_dir/`。
fn release_root(spec: &ReleaseBinSpec) -> PathBuf {
    if spec.dir_layout {
        bin_dir().join(spec.bin)
    } else {
        bin_dir()
    }
}

/// 目标二进制路径:
/// - zip: `<root>/<bin>` (Linux) / `<root>/<bin>.exe` (Windows), 解压后的可执行;
/// - 单文件: Linux `<root>/<linux_asset>` (资产名即本地文件名) / Windows `<root>/<bin>.exe`。
/// `<root>` = dir_layout ? `bin_dir/<bin>` : `bin_dir`。
fn release_bin_path(spec: &ReleaseBinSpec) -> PathBuf {
    let root = release_root(spec);
    if cfg!(windows) {
        root.join(format!("{}.exe", spec.bin))
    } else if spec.zip {
        root.join(spec.bin)
    } else {
        root.join(spec.linux_asset)
    }
}

/// 下载目标路径:
/// - Windows: 落 zip 资产名 (解压前先校验 sha256, 解压后删除);
/// - Linux: 落资产名 (zip 资产即 zip 本身, 单文件即二进制)。
fn release_download_path(spec: &ReleaseBinSpec) -> PathBuf {
    let root = release_root(spec);
    if cfg!(windows) {
        root.join(spec.windows_asset.unwrap_or(spec.linux_asset))
    } else {
        root.join(spec.linux_asset)
    }
}

/// 解压 Windows zip 资产到 bin_dir (zip 内文件平铺: `<bin>.exe` + 全部 dll)。
/// 只取 file_name, 防 zip-slip; 忽略目录条目; 已存在文件直接覆盖 (重新 ensure 时刷新)。
/// 解压 zip 平铺到 `dest_dir`: 只取各条目 file_name (忽略目录), 防 zip-slip (拒绝
/// `.`/`..`/空名), 已存在文件直接覆盖 (重新 ensure 时刷新)。平台无关, 便于单测。
fn extract_zip_flat(dest_dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let fname = Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .ok_or_else(|| format!("zip 条目非法: {name}"))?;
        let out = dest_dir.join(fname);
        let mut out_f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out_f).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 解压 zip 资产到指定落点根目录 (dir_layout spec 是 `bin_dir/<bin>`, 其余是 `bin_dir`)。
/// zip 内文件平铺 (只取 file_name 防 zip-slip): 二进制 + 全部运行时 dll/.so 同目录。
fn extract_zip_to_bin_dir(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    extract_zip_flat(dest_dir, zip_path)
}

/// 版本戳文件路径
fn release_version_path(spec: &ReleaseBinSpec) -> PathBuf {
    release_root(spec).join(spec.stamp)
}

/// 下载 URL (资产名与本地文件名一致)。
fn release_bin_url(spec: &ReleaseBinSpec, asset: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/{}/{}",
        spec.repo, spec.tag, asset
    )
}

/// 查上游最新 tag (用于「有新版可更新」提示)。
///
/// 列 `spec.repo` 的 releases，取产品名与 `spec.bin` 完全相等的最新 tag。
/// vision-lab 一个 repo 里放多个二进制的 release，按「产品名-v版本」切分，
/// 不能用前缀匹配（`subtitle-ocr-` 会误命中 `subtitle-ocr-post-*`）。
/// 任何失败 (网络 / 限流 / 无匹配) 一律返回 `None`：这是提示性信息,
/// 不该让 env 检查本身失败。
fn tag_product_matches(tag: &str, bin: &str) -> bool {
    let Some(pos) = tag.rfind("-v") else {
        return false;
    };
    let product = &tag[..pos];
    let version = &tag[pos + 2..];
    product == bin && version.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// 从 tag 列表里找 spec.bin 对应的最新 tag。
async fn fetch_latest_tag(spec: &ReleaseBinSpec) -> Option<String> {
    let url = format!("https://api.github.com/repos/{}/releases", spec.repo);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .ok()?;
    let mut req = client
        .get(&url)
        .header("User-Agent", "localdub-env-check")
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let releases: serde_json::Value = resp.json().await.ok()?;
    let mut tags = releases
        .as_array()?
        .iter()
        .filter_map(|r| r.get("tag_name")?.as_str());
    tags.find(|t| tag_product_matches(t, spec.bin))
        .map(|t| t.to_string())
}

/// 版本戳是否命中期望的 tag + sha256（纯函数，便于钉住认领门槛）。
fn version_stamp_matches(stamp: &Option<serde_json::Value>, tag: &str, sha256: &str) -> bool {
    stamp
        .as_ref()
        .and_then(|v| v.get("tag").and_then(|t| t.as_str()))
        == Some(tag)
        && stamp
            .as_ref()
            .and_then(|v| v.get("sha256").and_then(|s| s.as_str()))
            == Some(sha256)
}

/// 读取版本戳
fn read_version_stamp(path: &Path) -> Option<serde_json::Value> {
    if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    } else {
        None
    }
}

/// 写入版本戳
fn write_version_stamp(path: &Path, tag: &str, sha256: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stamp = serde_json::json!({
        "tag": tag,
        "sha256": sha256,
        "downloaded_at": chrono::Utc::now().to_rfc3339(),
    });
    std::fs::write(path, serde_json::to_string_pretty(&stamp)?)?;
    Ok(())
}

/// 计算文件 sha256
fn file_sha256(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hex::encode(hasher.finalize()))
}

async fn check_release_bin(spec: &ReleaseBinSpec) -> CheckResult {
    let Some((asset, sha256)) = current_platform_asset(spec) else {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("{} 暂无 {} 发布资产, 请等待 {} 发布或在此平台源码构建", spec.bin, platform_label(), spec.repo) }),
            required: false,
        };
    };
    // 版本戳校验先行：戳匹配 = 已认领/已下载并校验过，直接走常规检查，不再碰本地资产。
    // 戳不匹配时才有必要尝试认领——否则对 Linux 单文件 spec (dl_path == bin_path)，
    // 会把已装好的二进制当成"用户放的资产"再校验一次 sha，误伤正常安装。
    let stamp_path = release_version_path(spec);
    let stamp_ok = |stamp: &Option<serde_json::Value>| version_stamp_matches(stamp, spec.tag, sha256);

    // 认领前置：仅当版本戳未匹配时，看用户是否手动放进 bin_dir 一个合法资产
    // (sha256 命中 spec)。命中就就地认领 (解压 / 补执行位 / 写戳)，让 check 当场报
    // Pass，而不是先报缺失、等 ensure 偷偷认领——check 与 ensure 从此看到同一真相。
    // 资产存在但 sha 不符 → 认领返回 Fail，原样上报 (不覆盖用户文件)。
    if !stamp_ok(&read_version_stamp(&stamp_path)) {
        if let Some(adopted) = adopt_local_release(spec, sha256).await {
            if adopted.status == CheckStatus::Fail {
                return adopted;
            }
        }
    }

    let path = release_bin_path(spec);
    if !path.exists() {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("{} 二进制不存在", asset) }),
            required: false,
        };
    }

    // 执行位自愈 (仅 Linux): 下载/复制可能丢失 +x。若 check 走 Pass 分支, ensure_bin
    // 不会触发重新下载 (版本戳匹配), 这里直接补上执行位, 避免 spawn 报 EACCES。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode();
            let has_exec = mode & 0o111 != 0;
            if cfg!(target_os = "linux") && !has_exec {
                let mut perm = meta.permissions();
                perm.set_mode(mode | 0o700);
                std::fs::set_permissions(&path, perm).ok();
                tracing::info!(target: "sf_ocr", "{} 缺执行位, 已补 0o700", path.display());
            }
        }
    }

    // ldd 检查 (仅 Linux)。运行时 so 与二进制同目录 (dir_layout = bin_dir/<bin>,
    // 其余 = bin_dir), 需注入 LD_LIBRARY_PATH 指向该目录, 否则恒报 missing_libs。
    // 与 ensure_bin_command 的注入保持一致 (都取二进制 parent)。
    #[cfg(target_os = "linux")]
    {
        let lib_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let lib_path = format!(
            "{}:{}",
            lib_dir.display(),
            std::env::var("LD_LIBRARY_PATH").unwrap_or_default()
        );
        let mut cmd = Command::new("ldd");
        cmd.arg(path.as_os_str()).env("LD_LIBRARY_PATH", &lib_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let (ok, out, _) = match cmd.output() {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                (o.status.success(), stdout, stderr)
            }
            Err(_) => (false, String::new(), String::new()),
        };
        if ok && out.contains("not found") {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Warn,
                data: json!({ "path": path.display().to_string(), "runtime": "missing_libs", "msg": "动态库缺失 (ldd not found)" }),
                required: false,
            };
        }
    }

    // 版本戳校验（认领可能刚写过戳，这里重读）
    let stamp = read_version_stamp(&stamp_path);
    if !stamp_ok(&stamp) {
        let (tag_ok, sha_ok) = (
            stamp
                .as_ref()
                .and_then(|v| v.get("tag").and_then(|t| t.as_str()))
                == Some(spec.tag),
            stamp
                .as_ref()
                .and_then(|v| v.get("sha256").and_then(|s| s.as_str()))
                == Some(sha256),
        );
        let missing = match (tag_ok, sha_ok) {
            (false, false) => "版本戳缺失/不匹配",
            (false, true) => "tag 不匹配",
            (true, false) => "sha256 不匹配",
            _ => "版本不匹配",
        };
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Warn,
            data: json!({ "path": path.display().to_string(), "msg": missing, "stale": true }),
            required: false,
        };
    }

    // 已就绪：顺带问一下上游最新 tag，让 UI 能提示「有新版可更新」。
    // 查不到就是 None，不写这个字段（渲染侧会跳过）——提示性信息不该让 check 失败。
    // 仅当上游有新版本 (latest_tag != spec.tag) 时才写, 否则已是最新, 不提示。
    let mut data = json!({ "path": path.display().to_string(), "msg": "已就绪" });
    if let Some(latest) = fetch_latest_tag(spec).await {
        if latest != spec.tag {
            data["latest_tag"] = json!(latest);
        }
    }
    CheckResult {
        key: spec.key.to_string(),
        status: CheckStatus::Pass,
        data,
        required: false,
    }
}

/// 断点文件路径：`<asset>.part`，跨进程保留下载进度。
fn download_part_path(dl_path: &Path) -> PathBuf {
    let name = dl_path.file_name().unwrap_or_default().to_string_lossy();
    dl_path.with_file_name(format!("{name}.part"))
}

/// 带断点续传 + 有限重试的下载。网络中断时保留 `.part`，下次从已下字节继续；
/// 服务端忽略 Range（回 200）则从头重写。返回最终文件字节数。
async fn download_resumable(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    pb: &indicatif::ProgressBar,
) -> Result<u64, String> {
    const MAX_ATTEMPTS: usize = 8;
    let mut last_error = String::new();

    for attempt in 1..=MAX_ATTEMPTS {
        let existing = std::fs::metadata(part_path).map(|m| m.len()).unwrap_or(0);

        let mut req = client.get(url);
        if existing > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }

        let mut resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_error = format!("请求失败: {e}");
                tracing::warn!(target: "sf_ocr", "下载第 {attempt}/{MAX_ATTEMPTS} 次请求失败: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // 断点已超出文件（或文件变了）→ 清掉从头来
            let _ = std::fs::remove_file(part_path);
            continue;
        }
        if !status.is_success() {
            return Err(format!("下载失败: HTTP {status}"));
        }

        // 206 = 服务端接受 Range，从断点续写；200 = 从头重写。
        let append_mode = existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        let total = if append_mode {
            existing + resp.content_length().unwrap_or(0)
        } else {
            resp.content_length().unwrap_or(0)
        };
        if total > 0 {
            pb.set_length(total);
        }
        pb.set_position(existing);

        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append_mode)
            .truncate(!append_mode)
            .open(part_path)
        {
            Ok(f) => f,
            Err(e) => return Err(format!("打开断点文件失败: {e}")),
        };

        let mut written = 0u64;
        let mut interrupted: Option<String> = None;
        loop {
            match resp.chunk().await {
                Ok(Some(c)) => {
                    if let Err(e) = std::io::Write::write_all(&mut file, &c) {
                        interrupted = Some(format!("写入失败: {e}"));
                        break;
                    }
                    written += c.len() as u64;
                    pb.set_position(existing + written);
                }
                Ok(None) => break,
                Err(e) => {
                    interrupted = Some(format!("传输中断: {e}"));
                    break;
                }
            }
        }
        drop(file);

        if let Some(e) = interrupted {
            last_error = format!(
                "第 {attempt}/{MAX_ATTEMPTS} 次: {e}（断点已保留 {} 字节）",
                existing + written
            );
            tracing::warn!(target: "sf_ocr", "{last_error}");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }

        // 正常读完：字节数以文件实际长度为准（sha256 会做最终校验）
        return Ok(std::fs::metadata(part_path)
            .map(|m| m.len())
            .unwrap_or(existing + written));
    }

    Err(format!(
        "下载失败（重试 {MAX_ATTEMPTS} 次）: {last_error}（断点已保留，重跑可继续）"
    ))
}

/// 认领用户手动放到 bin_dir 的资产文件，返回 `Some(CheckResult)` 表示已就位/明确报错
/// （两种情况都不要再下载），`None` 表示没找到、走正常下载。
///
/// 资产文件（zip 或单文件二进制）存在 → 用 spec 的 sha256 校验。通过则就位
/// （zip 解压；单文件资产本身就是最终二进制，已在正确路径）、写戳认领；不符则
/// **报错保留用户文件**（不覆盖）。zip 资产若用户只放了已解压的二进制、没有
/// 压缩包可校验 sha，则不认领——无法确认它不是被篡改/不完整的版本。
async fn adopt_local_release(spec: &ReleaseBinSpec, sha256: &str) -> Option<CheckResult> {
    // dir_layout spec 优先认领新目录里的资产；若不存在则回退历史平铺位置
    // (bin_dir/<asset>)，让已有安装能从旧布局平滑迁移，sha 命中后解压进新目录。
    let dl_new = release_download_path(spec);
    let dl_path = if dl_new.exists() || !spec.dir_layout {
        dl_new
    } else {
        let legacy = bin_dir().join(
            if cfg!(windows) {
                spec.windows_asset.unwrap_or(spec.linux_asset)
            } else {
                spec.linux_asset
            },
        );
        if legacy.exists() { legacy } else { dl_new }
    };
    let bin_path = release_bin_path(spec);
    let stamp_path = release_version_path(spec);

    // 路径 1：资产文件存在（zip 或单文件）
    if dl_path.exists() {
        let actual = match file_sha256(&dl_path) {
            Ok(s) => s,
            Err(e) => {
                return Some(CheckResult {
                    key: spec.key.to_string(),
                    status: CheckStatus::Fail,
                    data: json!({ "msg": format!("本地资产 {} sha256 计算失败: {e}（未改动该文件）", dl_path.display()) }),
                    required: false,
                });
            }
        };
        if actual != sha256 {
            return Some(CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!(
                    "本地资产 {} sha256 不符：期望 {} 实际 {}（未覆盖，请检查你放的文件）",
                    dl_path.display(), sha256, actual
                ) }),
                required: false,
            });
        }
        // sha 通过：zip 解压就位（单文件已在正确路径）
        if spec.zip {
            let root = release_root(spec);
            if let Err(e) = std::fs::create_dir_all(&root) {
                return Some(CheckResult {
                    key: spec.key.to_string(),
                    status: CheckStatus::Fail,
                    data: json!({ "msg": format!("创建落点目录失败: {e}") }),
                    required: false,
                });
            }
            if let Err(e) = extract_zip_to_bin_dir(&dl_path, &root) {
                return Some(CheckResult {
                    key: spec.key.to_string(),
                    status: CheckStatus::Fail,
                    data: json!({ "msg": format!("解压本地资产失败: {e}") }),
                    required: false,
                });
            }
            let _ = std::fs::remove_file(&dl_path);
        }
        #[cfg(unix)]
        set_executable(&bin_path);
        if let Err(e) = write_version_stamp(&stamp_path, spec.tag, sha256) {
            return Some(CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("写版本戳失败: {e}") }),
                required: false,
            });
        }
        return Some(CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Pass,
            data: json!({ "path": bin_path.display().to_string(), "msg": "已认领本地资产（sha256 校验通过）" }),
            required: false,
        });
    }

    None
}

/// Linux 补执行位（下载/手动放置可能丢失 +x）。
#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode();
        if mode & 0o111 == 0 {
            let mut perm = meta.permissions();
            perm.set_mode(mode | 0o700);
            let _ = std::fs::set_permissions(path, perm);
        }
    }
}

async fn ensure_release_bin(spec: &ReleaseBinSpec) -> CheckResult {
    let Some((asset, sha256)) = current_platform_asset(spec) else {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("{} 暂无 {} 发布资产, 请等待 {} 发布或在此平台源码构建", spec.bin, platform_label(), spec.repo) }),
            required: false,
        };
    };
    // 空 sha256 = 该 release 尚未发布 (spec 先登记、发布后回填)。明确报未发布,
    // 不要拿着空校验值去下载。
    if sha256.is_empty() {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({
                "msg": format!("{} 尚未发布 ({} {} 无资产)", spec.bin, spec.repo, spec.tag),
            }),
            required: false,
        };
    }

    // 认领已前置到 check_release_bin: check 命中合法本地资产会就地认领并报 Pass,
    // run_ensure / ensure_bin 先跑 check 因而不会再走到这里。ensure 现在只负责下载。

    let bin_path = release_bin_path(spec);
    let dl_path = release_download_path(spec);
    if let Some(parent) = dl_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("创建目录失败: {e}") }),
                required: false,
            };
        }
    }

    let url = release_bin_url(spec, asset);
    tracing::info!(target: "sf_ocr", "正在下载 {} 从 {}", asset, url);

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("构建 HTTP 客户端失败: {e}") }),
                required: false,
            };
        }
    };

    let pb = indicatif::ProgressBar::new(0);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let part_path = download_part_path(&dl_path);
    if let Err(e) = download_resumable(&client, &url, &part_path, &pb).await {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": e }),
            required: false,
        };
    }
    pb.finish_with_message("下载完成");

    // 校验 sha256（对 .part 校验，通过后再改名为正式归档）
    let sha = match file_sha256(&part_path) {
        Ok(s) => s,
        Err(e) => {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("sha256 计算失败: {e}") }),
                required: false,
            };
        }
    };
    if sha != sha256 {
        // 内容不符：断点已无意义，删掉从头来
        let _ = std::fs::remove_file(&part_path);
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("sha256 校验失败: 期望 {} 实际 {}（已删除错误文件）", sha256, sha) }),
            required: false,
        };
    }

    // 校验通过：.part → 正式归档路径
    if let Err(e) = std::fs::rename(&part_path, &dl_path) {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("重命名下载文件失败: {e}") }),
            required: false,
        };
    }

    // zip 资产 (vision-lab Windows / vox-lab demucs Linux+Windows): 解压平铺到
    // 落点根目录 (dir_layout = bin_dir/<bin>, 其余 = bin_dir), 校验通过后删除归档。
    // 单文件资产 (Linux vision-lab) 跳过。
    if spec.zip {
        if let Err(e) = extract_zip_to_bin_dir(&dl_path, &release_root(spec)) {
            let _ = std::fs::remove_file(&dl_path);
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("解压失败: {e}") }),
                required: false,
            };
        }
        let _ = std::fs::remove_file(&dl_path);
    }

    // 执行权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perms) = std::fs::metadata(&bin_path).map(|m| m.permissions()) {
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perms).ok();
        }
    }

    // 写版本戳
    if let Err(e) = write_version_stamp(&release_version_path(spec), spec.tag, sha256) {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("写版本戳失败: {e}") }),
            required: false,
        };
    }

    CheckResult {
        key: spec.key.to_string(),
        status: CheckStatus::Pass,
        data: json!({ "path": bin_path.display().to_string(), "msg": "下载并校验成功" }),
        required: false,
    }
}

pub async fn check_subtitle_finder_bin() -> CheckResult {
    check_release_bin(&SUBTITLE_FINDER).await
}
pub async fn check_subtitle_ocr_bin() -> CheckResult {
    check_release_bin(&SUBTITLE_OCR).await
}
pub async fn check_subtitle_ocr_post_bin() -> CheckResult {
    check_release_bin(&SUBTITLE_OCR_POST).await
}
/// 当前平台的目标二进制路径 (供 `bin_path_from_key` 复用, 与下载路径保持一致)。
pub fn subtitle_finder_bin_path() -> PathBuf {
    release_bin_path(&SUBTITLE_FINDER)
}
pub fn subtitle_ocr_bin_path() -> PathBuf {
    release_bin_path(&SUBTITLE_OCR)
}
pub fn subtitle_ocr_post_bin_path() -> PathBuf {
    release_bin_path(&SUBTITLE_OCR_POST)
}

pub async fn check_demucs_burn_tch_bin() -> CheckResult {
    check_release_bin(&DEMUCS_BURN_TCH).await
}
pub async fn check_demucs_burn_wgpu_bin() -> CheckResult {
    check_release_bin(&DEMUCS_BURN_WGPU).await
}
pub async fn check_demucs_wgpu_bin() -> CheckResult {
    check_release_bin(&DEMUCS_WGPU).await
}
pub async fn check_demucs_tch_cpu_bin() -> CheckResult {
    check_release_bin(&DEMUCS_TCH_CPU).await
}
pub async fn check_demucs_tch_cuda_bin() -> CheckResult {
    check_release_bin(&DEMUCS_TCH_CUDA).await
}
/// 当前平台的目标二进制路径 (供 `bin_path_from_key` 复用, 与下载路径保持一致)。
pub fn demucs_burn_tch_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_BURN_TCH)
}
pub fn demucs_burn_wgpu_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_BURN_WGPU)
}
pub fn demucs_wgpu_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_WGPU)
}
pub fn demucs_tch_cpu_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_TCH_CPU)
}
pub fn demucs_tch_cuda_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_TCH_CUDA)
}

/// whisper-vulkan 目标二进制路径 (release 下载落到 bin_dir)。
pub fn whisper_vulkan_bin_path() -> PathBuf {
    release_bin_path(&WHISPER_VULKAN)
}
// ---------------------------------------------------------------------------
// ensure: dotenv
// ---------------------------------------------------------------------------

fn ensure_dotenv() -> CheckResult {
    let src = repo_root().join(".env.example");
    let dst = repo_root().join(".env");
    if dst.exists() {
        return CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Pass,
            data: json!({ "msg": ".env 已存在" }),
            required: false,
        };
    }
    if !src.exists() {
        return CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": ".env.example 不存在" }),
            required: false,
        };
    }
    match std::fs::copy(&src, &dst) {
        Ok(_) => CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Pass,
            data: json!({ "msg": "已复制 .env.example → .env" }),
            required: false,
        },
        Err(e) => CheckResult {
            key: "dotenv".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("复制失败: {e}") }),
            required: false,
        },
    }
}

// ---------------------------------------------------------------------------
// EnvKey 行为：身份 + 元数据 + check + 可选 ensure，集中调度
// ---------------------------------------------------------------------------

impl EnvKey {
    /// 执行该项的 check。desired 仅供 demucs_burn_bin 读取（其余项忽略）。
    pub async fn check(&self, desired: Option<&str>) -> CheckResult {
        match self {
            Self::Bun => check_bun(),
            Self::Python => check_python(),
            Self::Uv => check_uv(),
            Self::Ffmpeg => check_ffmpeg(),
            Self::Cargo => check_cargo(),
            Self::Vulkan => check_vulkan(),
            Self::Rocm => check_rocm(),
            Self::Cuda => check_cuda(),
            Self::WhisperGgml => check_whisper_ggml(),
            Self::WhisperVad => check_whisper_vad(),
            Self::DemucsPth => check_demucs_pth(),
            Self::Voxcpm2Onnx => check_voxcpm2_onnx(),
            Self::Voxcpm2Pth => check_voxcpm2_pth(),
            Self::WhisperBin => check_whisper_bin().await,
            Self::DemucsBurnBin => check_demucs_burn_bin(desired),
            Self::DemucsBurnTchBin => check_demucs_burn_tch_bin().await,
            Self::DemucsBurnWgpuBin => check_demucs_burn_wgpu_bin().await,
            Self::DemucsWgpuBin => check_demucs_wgpu_bin().await,
            Self::DemucsTchCpuBin => check_demucs_tch_cpu_bin().await,
            Self::DemucsTchCudaBin => check_demucs_tch_cuda_bin().await,
            Self::SubtitleFinderBin => check_subtitle_finder_bin().await,
            Self::SubtitleOcrBin => check_subtitle_ocr_bin().await,
            Self::SubtitleOcrPostBin => check_subtitle_ocr_post_bin().await,
            Self::Cmake => check_cmake(),
            Self::YtDlp => check_ytdlp(),
            Self::Git => check_git(),
            Self::Dotenv => check_dotenv(),
            Self::Openai => check_openai().await,
        }
    }

    /// 执行 ensure；None = 该项只能检测、没有安装动作。
    pub async fn ensure(&self) -> Option<CheckResult> {
        Some(match self {
            Self::Dotenv => ensure_dotenv(),
            Self::Openai => ensure_openai().await,
            Self::SubtitleFinderBin => ensure_release_bin(&SUBTITLE_FINDER).await,
            Self::SubtitleOcrBin => ensure_release_bin(&SUBTITLE_OCR).await,
            Self::SubtitleOcrPostBin => ensure_release_bin(&SUBTITLE_OCR_POST).await,
            Self::DemucsBurnTchBin => ensure_release_bin(&DEMUCS_BURN_TCH).await,
            Self::DemucsBurnWgpuBin => ensure_release_bin(&DEMUCS_BURN_WGPU).await,
            Self::DemucsWgpuBin => ensure_release_bin(&DEMUCS_WGPU).await,
            Self::DemucsTchCpuBin => ensure_release_bin(&DEMUCS_TCH_CPU).await,
            Self::DemucsTchCudaBin => ensure_release_bin(&DEMUCS_TCH_CUDA).await,
            Self::WhisperBin => ensure_release_bin(&WHISPER_VULKAN).await,
            _ => return None,
        })
    }

    /// 是否有安装动作（与 ensure 的 match 同一形状，避免跑一次 ensure 才知道）。
    pub fn has_ensure(&self) -> bool {
        matches!(
            self,
            Self::Dotenv
                | Self::Openai
                | Self::SubtitleFinderBin
                | Self::SubtitleOcrBin
                | Self::SubtitleOcrPostBin
                | Self::DemucsBurnTchBin
                | Self::DemucsBurnWgpuBin
                | Self::DemucsWgpuBin
                | Self::DemucsTchCpuBin
                | Self::DemucsTchCudaBin
                | Self::WhisperBin
        )
    }

    /// release 二进制的目标路径；非 release 项无路径。
    pub fn bin_path(&self) -> Option<PathBuf> {
        Some(match self {
            Self::SubtitleFinderBin => subtitle_finder_bin_path(),
            Self::SubtitleOcrBin => subtitle_ocr_bin_path(),
            Self::SubtitleOcrPostBin => subtitle_ocr_post_bin_path(),
            Self::DemucsBurnTchBin => demucs_burn_tch_bin_path(),
            Self::DemucsBurnWgpuBin => demucs_burn_wgpu_bin_path(),
            Self::DemucsWgpuBin => demucs_wgpu_bin_path(),
            Self::DemucsTchCpuBin => demucs_tch_cpu_bin_path(),
            Self::DemucsTchCudaBin => demucs_tch_cuda_bin_path(),
            Self::WhisperBin => whisper_vulkan_bin_path(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn version_stamp_matches_tag_and_sha() {
        let stamp = serde_json::json!({ "tag": "v1", "sha256": "abc", "downloaded_at": "t" });
        assert!(version_stamp_matches(&Some(stamp.clone()), "v1", "abc"));
        // tag 不匹配、sha 不匹配、戳缺失 → 都判为未认领，需要尝试认领
        assert!(!version_stamp_matches(&Some(stamp.clone()), "v2", "abc"));
        assert!(!version_stamp_matches(&Some(stamp), "v1", "def"));
        assert!(!version_stamp_matches(&None, "v1", "abc"));
        // 缺字段
        let partial = serde_json::json!({ "tag": "v1" });
        assert!(!version_stamp_matches(&Some(partial), "v1", "abc"));
    }

    #[test]
    fn tag_product_matching_does_not_confuse_ocr_and_ocr_post() {
        // 同一个 repo 里 ocr / ocr-post / finder 并存，前缀匹配会串味。
        assert!(tag_product_matches("subtitle-ocr-v0.1.1", "subtitle-ocr"));
        assert!(!tag_product_matches("subtitle-ocr-post-v0.1.3", "subtitle-ocr"));
        assert!(tag_product_matches("subtitle-ocr-post-v0.1.3", "subtitle-ocr-post"));
        assert!(tag_product_matches("subtitle-finder-v0.1.1", "subtitle-finder"));
        assert!(!tag_product_matches("subtitle-ocr-v0.1.1", "subtitle-finder"));
    }

    static TMP_ID: AtomicUsize = AtomicUsize::new(0);

    fn tmp_dir(tag: &str) -> PathBuf {
        let id = TMP_ID.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("ld_env_items_{}_{}", tag, id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 用 zip crate 合成 zip 到 `zip_path`。
    fn write_zip(zip_path: &Path, entries: &[(&str, &[u8])]) -> Result<(), String> {
        use zip::write::FileOptions;
        let file = std::fs::File::create(zip_path).map_err(|e| e.to_string())?;
        let mut zw = zip::ZipWriter::new(file);
        for (name, data) in entries {
            zw.start_file(*name, FileOptions::default())
                .map_err(|e| e.to_string())?;
            zw.write_all(data).map_err(|e| e.to_string())?;
        }
        zw.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn extract_zip_flat_flattens_and_skips_dirs() {
        let dir = tmp_dir("flat");
        let zip_path = dir.join("a.zip");
        // 顶层 exe + 子目录内 dll + 纯目录条目
        write_zip(
            &zip_path,
            &[
                ("subtitle-ocr.exe", b"exe-data"),
                ("nested/sub/dir/foo.dll", b"dll-data"),
                ("empty-dir/", b""),
            ],
        )
        .unwrap();

        extract_zip_flat(&dir, &zip_path).unwrap();

        // exe 平铺到 dest 根
        let exe = dir.join("subtitle-ocr.exe");
        assert!(exe.exists(), "exe 应解到目标根目录");
        assert_eq!(std::fs::read(&exe).unwrap(), b"exe-data");
        // 子目录内文件丢掉 目录前缀, 直接平铺到根
        let dll = dir.join("foo.dll");
        assert!(dll.exists(), "深层条目应平铺到根");
        assert_eq!(std::fs::read(&dll).unwrap(), b"dll-data");
        // 纯目录条目被跳过, 不产生 empty-dir 文件
        assert!(!dir.join("empty-dir").exists());
    }

    #[test]
    fn extract_zip_flat_flattens_escaping_paths_safely() {
        let dir = tmp_dir("escape");
        let zip_path = dir.join("b.zip");
        // 含 ../ 或绝对路径的条目: file_name() 会剥离目录前缀, 只平铺 basename, 无法逃逸。
        write_zip(
            &zip_path,
            &[("../../evil.exe", b"evil"), ("/abs/path.dll", b"abs")],
        )
        .unwrap();

        extract_zip_flat(&dir, &zip_path).unwrap();
        // 全部落在目标目录内, 没有逃逸到临时目录上层
        assert_eq!(std::fs::read(dir.join("evil.exe")).unwrap(), b"evil");
        assert_eq!(std::fs::read(dir.join("path.dll")).unwrap(), b"abs");
        let parent = dir.parent().unwrap();
        assert!(!parent.join("evil.exe").exists(), "不得逃逸到上级目录");
        assert!(!parent.join("path.dll").exists(), "不得逃逸到上级目录");
    }

    #[test]
    fn extract_zip_flat_rejects_dotdot_basename() {
        let dir = tmp_dir("dotdot");
        let zip_path = dir.join("c.zip");
        // basename 就是 .. (如条目名 "..") → 拒绝
        write_zip(&zip_path, &[("..", b"x")]).unwrap();
        let err = extract_zip_flat(&dir, &zip_path).unwrap_err();
        assert!(
            err.contains("zip 条目非法"),
            "应拒绝 => basename, got: {err}"
        );
    }

    #[test]
    fn extract_zip_flat_overwrites_existing() {
        let dir = tmp_dir("overwrite");
        std::fs::write(dir.join("subtitle-ocr.exe"), b"stale").unwrap();
        let zip_path = dir.join("d.zip");
        write_zip(&zip_path, &[("subtitle-ocr.exe", b"fresh")]).unwrap();

        extract_zip_flat(&dir, &zip_path).unwrap();
        assert_eq!(
            std::fs::read(dir.join("subtitle-ocr.exe")).unwrap(),
            b"fresh"
        );
    }

    #[test]
    fn try_exec_returns_trimmed_output_on_success() {
        // 快速命令正常返回, 输出去空白 (镜像 TS tryExec)
        let (ok, out, _) = try_exec("bun", &["--version"], None);
        // bun 可能未安装; 该场景走 false 分支, 不影响其它断言
        if ok {
            assert!(!out.is_empty());
        }

        let (ok, out, _) = try_exec("cargo", &["--version"], None);
        if ok {
            assert!(out.contains("cargo"));
        }
    }

    #[test]
    fn try_exec_times_out_and_kills_hung_command() {
        // 慢命令: sleep 30 > TRY_EXEC_TIMEOUT(10s) → 超时 kill, ok=false
        // 验证超时路径真生效 (而非卡死)。unix-only (sleep 命令)。
        let start = std::time::Instant::now();
        let (ok, out, _) = try_exec("sleep", &["30"], None);
        let elapsed = start.elapsed();
        assert!(!ok, "超时应返回 false");
        assert!(out.is_empty());
        assert!(
            elapsed < std::time::Duration::from_secs(15),
            "应在超时窗口内返回, 实际 {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn fetch_latest_tag_matches_bin_prefix() {
        // 网络测试: 有 token 且可达时才断言 (匿名常 403 限流; 网络不可达静默跳过)。
        // 验证 fetch_latest_tag 按 `{bin}-` 前缀精确匹配, 不跨二进制误报:
        // vision-lab 一个 repo 含 subtitle-ocr / subtitle-ocr-post / subtitle-finder 多个 release。
        let Ok(token) = std::env::var("GITHUB_TOKEN") else {
            return;
        };
        if token.trim().is_empty() {
            return;
        }
        let Some(finder) = fetch_latest_tag(&SUBTITLE_FINDER).await else {
            return;
        };
        assert!(
            finder.starts_with("subtitle-finder-"),
            "finder 应精确命中 subtitle-finder-* tag, got {finder}"
        );
        assert!(
            !finder.starts_with("subtitle-ocr"),
            "finder 不应命中 subtitle-ocr 系: {finder}"
        );
    }
}

