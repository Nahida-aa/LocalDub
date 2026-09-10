//! 环境检测的具体检查项 (镜像 TS `packages/core/cmd/env/items.ts`)。
//!
//! 设计抉择 (见 plan):
//! - i18n: TS 用 `@repo/shared/i18n` 的 `t(key)`; Rust 无框架, 各 check 在 `data`
//!   里给出简化 `msg` 字段, `format_result` 直接打印 + 中文描述 (input::zh_desc)。
//! - try_exec 超时: TS 用 `spawnSync` 10s 超时; Rust 用 spawn + try_wait 轮询 +
//!   超时 kill (`TRY_EXEC_TIMEOUT`), 超时视为 ok=false (对齐 TS)。
//! - ollama 分离进程: 用 `std::process::Command` + `Stdio::null()` + unix
//!   `process_group(0)` (win 用 `CREATE_NEW_PROCESS_GROUP`) 替代 `spawn detached`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;
use chrono;

use config_rs::env::{openai_api_key, openai_base_url};
use config_rs::path::models::{bin_dir, demucs_model_dir, voxcpm_model_dir, whisper_model_dir};
use config_rs::root::repo_root;

use crate::cmd::env::{CheckResult, CheckStatus};

// ---------------------------------------------------------------------------
// 本地 helper
// ---------------------------------------------------------------------------

/// 镜像 TS `tryExec` (spawnSync timeout:10s): 同步执行命令, 超时 kill 视为 ok=false。
fn try_exec(cmd: &str, args: &[&str], cwd: Option<&Path>) -> (bool, String, String) {
    use std::io::Read;
    use std::process::Stdio;

    let mut c = Command::new(cmd);
    c.args(args);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(_) => return (false, String::new(), String::new()),
    };
    let deadline = std::time::Instant::now() + TRY_EXEC_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // 退出后用剩余 stdout/stderr 管道读取完整输出 (与 TS trim 语义一致)
                let mut stdout = String::new();
                let mut stderr = String::new();
                let _ = child
                    .stdout
                    .take()
                    .map(|mut h| h.read_to_string(&mut stdout));
                let _ = child
                    .stderr
                    .take()
                    .map(|mut h| h.read_to_string(&mut stderr));
                return (
                    status.success(),
                    stdout.trim().to_string(),
                    stderr.trim().to_string(),
                );
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, String::new(), String::new());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return (false, String::new(), String::new()),
        }
    }
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
pub fn check_whisper_bin() -> CheckResult {
    check_release_bin(&WHISPER_VULKAN)
}

/// 检查 burn 系二进制。
pub fn check_demucs_burn_bin(required: Option<&str>) -> CheckResult {
    // demucs-burn 已迁至 vox-lab, 仅 tch/wgpu 有 release 资产 (经 demucs_burn_{tch,wgpu}_bin
    // 检查)。cpu/cuda/vulkan/rocm 等后端暂无发布资产, 源码构建路径也已移除。
    let _ = required;
    CheckResult {
        key: "demucs_burn_bin".into(),
        status: CheckStatus::Fail,
        data: json!({ "msg": "demucs-burn 仅发布 tch/wgpu 后端, 请切换 separate.runtime/device 到 burn-tch (tch) 或 burn+webgpu (wgpu)" }),
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

pub fn check_demucs_pth() -> CheckResult {
    check_model(
        &demucs_model_dir().join("htdemucs_ft.safetensors"),
        "demucs_pth",
        300.0,
    )
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

pub fn check_openai() -> CheckResult {
    // TS 读 `process.env.OPENAI_BASE_URL` (无默认); 这里同样读原始 env, 缺失即 fail。
    let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| openai_base_url());
    let api_key = openai_api_key();
    if (base_url.is_empty() || api_key.is_none())
        && !base_url.contains("localhost")
        && !base_url.contains("127.0.0.1")
    {
        return CheckResult {
            key: "openai".into(),
            status: CheckStatus::Fail,
            data: json!({ "issues": "不存在", "msg": "OPENAI_BASE_URL / OPENAI_API_KEY 未配置" }),
            required: false,
        };
    }

    let is_local = base_url.contains("localhost") || base_url.contains("127.0.0.1");
    let client = reqwest::blocking::Client::builder()
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
            .bearer_auth(api_key.clone().unwrap_or_default())
    };

    match req.send() {
        Ok(res) if res.status().is_success() => {
            let models = res
                .json::<serde_json::Value>()
                .ok()
                .and_then(|j| j.get("data").and_then(|d| d.as_array()).map(|a| a.len()))
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            CheckResult {
                key: "openai".into(),
                status: CheckStatus::Pass,
                data: json!({ "baseUrl": base_url, "models": format!("{models} models"), "msg": format!("可达 ({} models)", models) }),
                required: false,
            }
        }
        Ok(res) => CheckResult {
            key: "openai".into(),
            status: CheckStatus::Warn,
            data: json!({ "issues": format!("HTTP {}", res.status()), "msg": format!("HTTP {}", res.status()) }),
            required: false,
        },
        Err(e) => CheckResult {
            key: "openai".into(),
            status: CheckStatus::Fail,
            data: json!({ "issues": e.to_string(), "msg": format!("连接失败: {}", e) }),
            required: false,
        },
    }
}

fn ensure_openai() -> CheckResult {
    let base_url = openai_base_url();
    if !base_url.contains("localhost") && !base_url.contains("127.0.0.1") {
        return CheckResult {
            key: "openai".into(),
            status: CheckStatus::Skip,
            data: json!({ "issues": "not a local server", "baseUrl": base_url, "msg": "非本地服务, 跳过" }),
            required: false,
        };
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build();
    if let Ok(c) = client {
        if let Ok(res) = c.get(format!("{base_url}/models")).send() {
            if res.status().is_success() {
                return CheckResult {
                    key: "openai".into(),
                    status: CheckStatus::Pass,
                    data: json!({ "baseUrl": base_url, "msg": "已在运行" }),
                    required: false,
                };
            }
        }
    }

    // 用 ollama serve 拉起本地服务
    let ollama_bin = which("ollama");
    let Some(ollama_bin) = ollama_bin else {
        return CheckResult {
            key: "openai".into(),
            status: CheckStatus::Fail,
            data: json!({ "issues": "ollama not found in PATH", "msg": "未找到 ollama" }),
            required: false,
        };
    };

    spawn_detached(&ollama_bin, &["serve"]);

    let poll = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build();
    if let Ok(c) = poll {
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if let Ok(res) = c.get(format!("{base_url}/models")).send() {
                if res.status().is_success() {
                    return CheckResult {
                        key: "openai".into(),
                        status: CheckStatus::Pass,
                        data: json!({ "baseUrl": base_url, "msg": "ollama 已启动" }),
                        required: false,
                    };
                }
            }
        }
    }

    CheckResult {
        key: "openai".into(),
        status: CheckStatus::Fail,
        data: json!({ "issues": "ollama serve did not respond after 15s", "msg": "ollama 启动超时" }),
        required: false,
    }
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

/// 分离进程启动 (镜像 `spawn detached + unref`), 用于 ollama serve。
fn spawn_detached(bin: &str, args: &[&str]) {
    let mut c = Command::new(bin);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let _ = c.spawn();
}

// ---------------------------------------------------------------------------
// vision-lab release 二进制 (subtitle-finder / subtitle-ocr / ocr-post):
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
}

const SUBTITLE_FINDER: ReleaseBinSpec = ReleaseBinSpec {
    key: "subtitle_finder_bin",
    bin: "subtitle-finder",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-finder-v0.1.0",
    zip: false,
    linux_asset: "subtitle-finder-x86_64-unknown-linux-gnu",
    linux_sha256: "b08778b2e066a35f8c9b3c0457e3e05a1379a6452341b932d82c22175cba9923",
    windows_asset: Some("subtitle-finder-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("9dece5e3cd2a9d72716d5fab5ecec256406b5113e38a479f55c76c43635396cc"),
    stamp: ".subtitle_finder.version.json",
};

const SUBTITLE_OCR: ReleaseBinSpec = ReleaseBinSpec {
    key: "subtitle_ocr_bin",
    bin: "subtitle-ocr",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-ocr-v0.1.0",
    zip: false,
    linux_asset: "subtitle-ocr-x86_64-unknown-linux-gnu",
    linux_sha256: "5e4dc400e52fd9b9759d9a4e8a5714aa0622078cd8a52a7035178d8bd91ba6ca",
    windows_asset: Some("subtitle-ocr-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("bd2880bc2d7e63383fbd580b69619631343298d71fa037bf97fc976a8733e079"),
    stamp: ".subtitle_ocr.version.json",
};

const OCR_POST: ReleaseBinSpec = ReleaseBinSpec {
    key: "ocr_post_bin",
    bin: "ocr-post",
    repo: "Nahida-aa/vision-lab",
    tag: "subtitle-ocr-v0.1.0",
    zip: false,
    linux_asset: "ocr-post-x86_64-unknown-linux-gnu",
    linux_sha256: "107187c94051c8fda46f2fc18d6c6e8835593caa4fa8703a9fc3b41d1473a101",
    windows_asset: Some("ocr-post-x86_64-pc-windows-msvc.zip"),
    windows_sha256: Some("a2aaeda6cd4cc8861a6c5747216bf95361250bb32c86a8f19a8187eb888c0e2d"),
    stamp: ".ocr_post.version.json",
};

const DEMUCS_BURN_TCH: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_burn_tch_bin",
    bin: "demucs-burn-tch",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-burn-v0.1.0",
    zip: true,
    linux_asset: "demucs-burn-tch-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "3cd530a17dedbff53c4ee86befd2567621e3d3a335b93f2cf313ebe414a79308",
    windows_asset: None,
    windows_sha256: None,
    stamp: ".demucs_burn_tch.version.json",
};

const DEMUCS_BURN_WGPU: ReleaseBinSpec = ReleaseBinSpec {
    key: "demucs_burn_wgpu_bin",
    bin: "demucs-burn-wgpu",
    repo: "Nahida-aa/vox-lab",
    tag: "demucs-burn-v0.1.0",
    zip: true,
    linux_asset: "demucs-burn-wgpu-x86_64-unknown-linux-gnu.zip",
    linux_sha256: "f55bf5a80ae6fe9155df68eb652c70bceab422045eb3d3dbca703f9ec5c310e8",
    windows_asset: None,
    windows_sha256: None,
    stamp: ".demucs_burn_wgpu.version.json",
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
    windows_asset: None,
    windows_sha256: None,
    stamp: ".whisper_vulkan.version.json",
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

/// 目标二进制路径:
/// - zip: `bin_dir/<bin>` (Linux) / `bin_dir/<bin>.exe` (Windows), 解压平铺后的可执行;
/// - 单文件: Linux `bin_dir/<linux_asset>` (资产名即本地文件名) / Windows `bin_dir/<bin>.exe`。
fn release_bin_path(spec: &ReleaseBinSpec) -> PathBuf {
    if cfg!(windows) {
        bin_dir().join(format!("{}.exe", spec.bin))
    } else if spec.zip {
        bin_dir().join(spec.bin)
    } else {
        bin_dir().join(spec.linux_asset)
    }
}

/// 下载目标路径:
/// - Windows: 落 zip 资产名 (解压前先校验 sha256, 解压后删除);
/// - Linux: 落资产名 (zip 资产即 zip 本身, 单文件即二进制)。
fn release_download_path(spec: &ReleaseBinSpec) -> PathBuf {
    if cfg!(windows) {
        bin_dir().join(spec.windows_asset.unwrap_or(spec.linux_asset))
    } else {
        bin_dir().join(spec.linux_asset)
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

/// 解压 zip 资产 (vision-lab Windows zip / vox-lab demucs Linux+Windows zip) 平铺到 bin_dir。
/// 已知的 demucs 类 zip 顶层即为 bin_dir 内容, 故解压到 bin_dir 而非其子目录。
fn extract_zip_to_bin_dir(zip_path: &Path) -> Result<(), String> {
    extract_zip_flat(&bin_dir(), zip_path)
}

/// 版本戳文件路径
fn release_version_path(spec: &ReleaseBinSpec) -> PathBuf {
    bin_dir().join(spec.stamp)
}

/// 下载 URL (资产名与本地文件名一致)。
fn release_bin_url(spec: &ReleaseBinSpec, asset: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/{}/{}",
        spec.repo, spec.tag, asset
    )
}

/// 读取版本戳
fn read_version_stamp(path: &Path) -> Option<serde_json::Value> {
    if path.exists() {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
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

fn check_release_bin(spec: &ReleaseBinSpec) -> CheckResult {
    let Some((asset, sha256)) = current_platform_asset(spec) else {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("{} 暂无 {} 发布资产, 请等待 {} 发布或在此平台源码构建", spec.bin, platform_label(), spec.repo) }),
            required: false,
        };
    };
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

    // ldd 检查 (仅 Linux)。zip 平铺类 spec (demucs release) 的 libtorch 等 so 与 bin 同目录 (bin_dir),
    // 需注入 LD_LIBRARY_PATH 才能命中, 否则 tch 恒报 missing_libs。
    #[cfg(target_os = "linux")]
    {
        let bin_dir = bin_dir();
        let lib_path = format!("{}:{}", bin_dir.display(), std::env::var("LD_LIBRARY_PATH").unwrap_or_default());
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

    // 版本戳校验
    let stamp = read_version_stamp(&release_version_path(spec));
    let tag_ok = stamp.as_ref().and_then(|v| v.get("tag").and_then(|t| t.as_str())) == Some(spec.tag);
    let sha_ok = stamp.as_ref().and_then(|v| v.get("sha256").and_then(|s| s.as_str())) == Some(sha256);

    if !tag_ok || !sha_ok {
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

    CheckResult {
        key: spec.key.to_string(),
        status: CheckStatus::Pass,
        data: json!({ "path": path.display().to_string(), "msg": "已就绪" }),
        required: false,
    }
}

fn ensure_release_bin(spec: &ReleaseBinSpec) -> CheckResult {
    let Some((asset, sha256)) = current_platform_asset(spec) else {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("{} 暂无 {} 发布资产, 请等待 {} 发布或在此平台源码构建", spec.bin, platform_label(), spec.repo) }),
            required: false,
        };
    };
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

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build() {
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

    let mut resp = match client.get(url).send() {
        Ok(r) => r,
        Err(e) => {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("下载请求失败: {e}") }),
                required: false,
            };
        }
    };

    if !resp.status().is_success() {
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("下载失败: HTTP {}", resp.status()) }),
            required: false,
        };
    }

    let total = resp.content_length().unwrap_or(0);
    let pb = indicatif::ProgressBar::new(total);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut file = match std::fs::File::create(&dl_path) {
        Ok(f) => f,
        Err(e) => {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("创建文件失败: {e}") }),
                required: false,
            };
        }
    };

    let mut downloaded = 0u64;
    let mut buf = [0u8; 8192];
    while let Ok(n) = std::io::Read::read(&mut resp, &mut buf) {
        if n == 0 {
            break;
        }
        if let Err(e) = std::io::Write::write_all(&mut file, &buf[..n]) {
            return CheckResult {
                key: spec.key.to_string(),
                status: CheckStatus::Fail,
                data: json!({ "msg": format!("写入失败: {e}") }),
                required: false,
            };
        }
        downloaded += n as u64;
        pb.set_position(downloaded);
    }
    pb.finish_with_message("下载完成");

    // 校验 sha256
    let sha = match file_sha256(&dl_path) {
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
        let _ = std::fs::remove_file(&dl_path);
        return CheckResult {
            key: spec.key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "msg": format!("sha256 校验失败: 期望 {} 实际 {}", sha256, sha) }),
            required: false,
        };
    }

    // zip 资产 (vision-lab Windows / vox-lab demucs Linux+Windows): 解压平铺到
    // bin_dir, 校验通过后删除归档。单文件资产 (Linux vision-lab) 跳过。
    if spec.zip {
        if let Err(e) = extract_zip_to_bin_dir(&dl_path) {
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

pub fn check_subtitle_finder_bin() -> CheckResult {
    check_release_bin(&SUBTITLE_FINDER)
}
pub fn check_subtitle_ocr_bin() -> CheckResult {
    check_release_bin(&SUBTITLE_OCR)
}
pub fn check_ocr_post_bin() -> CheckResult {
    check_release_bin(&OCR_POST)
}
fn ensure_subtitle_finder_bin() -> CheckResult {
    ensure_release_bin(&SUBTITLE_FINDER)
}
fn ensure_subtitle_ocr_bin() -> CheckResult {
    ensure_release_bin(&SUBTITLE_OCR)
}
fn ensure_ocr_post_bin() -> CheckResult {
    ensure_release_bin(&OCR_POST)
}

/// 当前平台的目标二进制路径 (供 `bin_path_from_key` 复用, 与下载路径保持一致)。
pub fn subtitle_finder_bin_path() -> PathBuf {
    release_bin_path(&SUBTITLE_FINDER)
}
pub fn subtitle_ocr_bin_path() -> PathBuf {
    release_bin_path(&SUBTITLE_OCR)
}
pub fn ocr_post_bin_path() -> PathBuf {
    release_bin_path(&OCR_POST)
}

pub fn check_demucs_burn_tch_bin() -> CheckResult {
    check_release_bin(&DEMUCS_BURN_TCH)
}
pub fn check_demucs_burn_wgpu_bin() -> CheckResult {
    check_release_bin(&DEMUCS_BURN_WGPU)
}
fn ensure_demucs_burn_tch_bin() -> CheckResult {
    ensure_release_bin(&DEMUCS_BURN_TCH)
}
fn ensure_demucs_burn_wgpu_bin() -> CheckResult {
    ensure_release_bin(&DEMUCS_BURN_WGPU)
}

/// 当前平台的目标二进制路径 (供 `bin_path_from_key` 复用, 与下载路径保持一致)。
pub fn demucs_burn_tch_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_BURN_TCH)
}
pub fn demucs_burn_wgpu_bin_path() -> PathBuf {
    release_bin_path(&DEMUCS_BURN_WGPU)
}

/// whisper-vulkan 目标二进制路径 (release 下载落到 bin_dir)。
pub fn whisper_vulkan_bin_path() -> PathBuf {
    release_bin_path(&WHISPER_VULKAN)
}
fn ensure_whisper_bin() -> CheckResult {
    ensure_release_bin(&WHISPER_VULKAN)
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
// 调度表
// ---------------------------------------------------------------------------

/// 全部检查函数 (镜像 TS `allChecks`)。
pub fn all_checks() -> HashMap<&'static str, fn() -> CheckResult> {
    let mut m: HashMap<&'static str, fn() -> CheckResult> = HashMap::new();
    m.insert("bun", check_bun);
    m.insert("python", check_python);
    m.insert("uv", check_uv);
    m.insert("ffmpeg", check_ffmpeg);
    m.insert("cargo", check_cargo);
    m.insert("vulkan", check_vulkan);
    m.insert("rocm", check_rocm);
    m.insert("cuda", check_cuda);
    m.insert("whisper_ggml", check_whisper_ggml);
    m.insert("whisper_vad", check_whisper_vad);
    m.insert("demucs_pth", check_demucs_pth);
    m.insert("voxcpm2_onnx", check_voxcpm2_onnx);
    m.insert("voxcpm2_pth", check_voxcpm2_pth);
    m.insert("whisper_bin", check_whisper_bin);
    m.insert("demucs_burn_bin", || check_demucs_burn_bin(None));
    m.insert("subtitle_finder_bin", check_subtitle_finder_bin);
    m.insert("subtitle_ocr_bin", check_subtitle_ocr_bin);
    m.insert("ocr_post_bin", check_ocr_post_bin);
    m.insert("demucs_burn_tch_bin", check_demucs_burn_tch_bin);
    m.insert("demucs_burn_wgpu_bin", check_demucs_burn_wgpu_bin);
    m.insert("cmake", check_cmake);
    m.insert("git", check_git);
    m.insert("dotenv", check_dotenv);
    m.insert("openai", check_openai);
    m
}

/// 可 ensure 的项 (镜像 TS `ensureFns`)。
pub fn ensure_fns() -> HashMap<&'static str, fn() -> CheckResult> {
    let mut m: HashMap<&'static str, fn() -> CheckResult> = HashMap::new();
    m.insert("dotenv", ensure_dotenv);
    m.insert("openai", ensure_openai);
    m.insert("subtitle_finder_bin", ensure_subtitle_finder_bin);
    m.insert("subtitle_ocr_bin", ensure_subtitle_ocr_bin);
    m.insert("ocr_post_bin", ensure_ocr_post_bin);
    m.insert("demucs_burn_tch_bin", ensure_demucs_burn_tch_bin);
    m.insert("demucs_burn_wgpu_bin", ensure_demucs_burn_wgpu_bin);
    m.insert("whisper_bin", ensure_whisper_bin);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            zw.start_file(*name, FileOptions::default()).map_err(|e| e.to_string())?;
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
        write_zip(&zip_path, &[("../../evil.exe", b"evil"), ("/abs/path.dll", b"abs")]).unwrap();

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
        assert!(err.contains("zip 条目非法"), "应拒绝 => basename, got: {err}");
    }

    #[test]
    fn extract_zip_flat_overwrites_existing() {
        let dir = tmp_dir("overwrite");
        std::fs::write(dir.join("subtitle-ocr.exe"), b"stale").unwrap();
        let zip_path = dir.join("d.zip");
        write_zip(&zip_path, &[("subtitle-ocr.exe", b"fresh")]).unwrap();

        extract_zip_flat(&dir, &zip_path).unwrap();
        assert_eq!(std::fs::read(dir.join("subtitle-ocr.exe")).unwrap(), b"fresh");
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
}
