//! 环境检测的具体检查项 (镜像 TS `packages/core/cmd/env/items.ts`)。
//!
//! 设计抉择 (见 plan):
//! - i18n: TS 用 `@repo/shared/i18n` 的 `t(key)`; Rust 无框架, 各 check 在 `data`
//!   里给出简化 `msg` 字段, `format_result` 直接打印 + 中文描述 (input::zh_desc)。
//! - try_exec 超时: TS 用 `spawnSync` 10s 超时; Rust 用 `output()` 同步阻塞
//!   (不实现精确超时, 标注 TODO, 10s 阻塞可接受)。
//! - ollama 分离进程: 用 `std::process::Command` + `Stdio::null()` + unix
//!   `process_group(0)` (win 用 `CREATE_NEW_PROCESS_GROUP`) 替代 `spawn detached`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use config_rs::env::{openai_api_key, openai_base_url};
use config_rs::path::models::{demucs_model_dir, voxcpm_model_dir, whisper_model_dir};
use config_rs::root::repo_root;

use crate::cmd::env::{CheckResult, CheckStatus};

// ---------------------------------------------------------------------------
// 本地 helper
// ---------------------------------------------------------------------------

/// 同步执行命令 (镜像 TS `tryExec`)。TODO: 实现精确 10s 超时 (目前同步阻塞)。
fn try_exec(cmd: &str, args: &[&str], cwd: Option<&Path>) -> (bool, String, String) {
    let mut c = Command::new(cmd);
    c.args(args);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    match c.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            (out.status.success(), stdout, stderr)
        }
        Err(_) => (false, String::new(), String::new()),
    }
}

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

/// 取文件 mtime (秒, 截断为整秒, 与 git commit 时间可比)。
fn mtime_sec(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// 取 `path` 在 git 中的最近提交时间 (秒), 无则 None。
fn git_commit_time(repo: &Path, path: &str) -> Option<u64> {
    let (ok, out, _) = try_exec(
        "git",
        &["log", "-1", "--format=%ct", "--", path],
        Some(repo),
    );
    if !ok {
        return None;
    }
    out.trim().parse::<u64>().ok()
}

/// 二进制是否已过时 (源码 git 提交时间 > 二进制 mtime)。镜像 TS `isStale`。
fn is_stale(bin_path: &Path, watch_paths: &[&str]) -> bool {
    let Some(bin_time) = mtime_sec(bin_path) else {
        return false;
    };
    let repo = repo_root();
    for p in watch_paths {
        let Some(src_time) = git_commit_time(&repo, p) else {
            continue;
        };
        if src_time > bin_time {
            return true;
        }
    }
    false
}

/// 取多个 watch_paths 中最新的 git 提交时间。
fn get_latest_source(watch_paths: &[&str]) -> Option<u64> {
    let repo = repo_root();
    let mut latest = 0u64;
    for p in watch_paths {
        if let Some(t) = git_commit_time(&repo, p) {
            if t > latest {
                latest = t;
            }
        }
    }
    if latest == 0 { None } else { Some(latest) }
}

/// 模型大小检查 (镜像 TS `checkModel`)。min_mb 支持小数 (如 silero vad 0.5MB)。
fn check_model(path: &Path, key: &str, min_mb: f64) -> CheckResult {
    match file_size(path) {
        None => CheckResult {
            key: key.to_string(),
            status: CheckStatus::Fail,
            data: json!({}),
            required: false,
        },
        Some(size) => {
            let mb = size as f64 / 1e6;
            if mb < min_mb {
                CheckResult {
                    key: key.to_string(),
                    status: CheckStatus::Warn,
                    data: json!({ "size": fmt_size(size), "msg": format!("模型偏小: {}", fmt_size(size)) }),
                    required: false,
                }
            } else {
                CheckResult {
                    key: key.to_string(),
                    status: CheckStatus::Pass,
                    data: json!({ "size": fmt_size(size), "msg": format!("大小 {}", fmt_size(size)) }),
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

pub fn check_vcpkg() -> CheckResult {
    if !cfg!(windows) {
        return CheckResult {
            key: "vcpkg".into(),
            status: CheckStatus::Skip,
            data: json!({}),
            required: false,
        };
    }
    let git_dir = repo_root().join("submodule").join("vcpkg").join(".git");
    if !git_dir.exists() {
        return CheckResult {
            key: "vcpkg".into(),
            status: CheckStatus::Fail,
            data: json!({ "kind": "submodule", "msg": "vcpkg 子模块未初始化" }),
            required: false,
        };
    }
    let vcpkg_exe = repo_root()
        .join("submodule")
        .join("vcpkg")
        .join("vcpkg.exe");
    let (ok, _, _) = try_exec(vcpkg_exe.to_str().unwrap(), &["--version"], None);
    if !ok {
        return CheckResult {
            key: "vcpkg".into(),
            status: CheckStatus::Fail,
            data: json!({ "kind": "bootstrap", "msg": "vcpkg 未编译 (需 bootstrap)" }),
            required: false,
        };
    }
    CheckResult {
        key: "vcpkg".into(),
        status: CheckStatus::Pass,
        data: json!({}),
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

fn check_submodule(rel: &str, key: &str) -> CheckResult {
    let ok = repo_root().join(rel).join(".git").exists();
    CheckResult {
        key: key.to_string(),
        status: if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        data: json!({ "msg": if ok { "已初始化" } else { "子模块未初始化" } }),
        required: false,
    }
}

pub fn check_submodule_whisper_cpp() -> CheckResult {
    check_submodule("submodule/whisper.cpp", "submodule_whisper_cpp")
}
pub fn check_submodule_demucs_cpp() -> CheckResult {
    check_submodule("submodule/demucs.cpp", "submodule_demucs_cpp")
}
pub fn check_submodule_demucs_rs() -> CheckResult {
    check_submodule("submodule/demucs-rs", "submodule_demucs_rs")
}
pub fn check_submodule_voxcpm_rs() -> CheckResult {
    check_submodule("submodule/voxcpm-rs", "submodule_voxcpm_rs")
}

// ---------------------------------------------------------------------------
// 编译产物检查
// ---------------------------------------------------------------------------

pub fn check_whisper_bin() -> CheckResult {
    let path = config_rs::path::models::whisper_vulkan_path();
    if !path.exists() {
        return CheckResult {
            key: "whisper_bin".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "whisper-vulkan 未编译" }),
            required: false,
        };
    }
    let stale = is_stale(&path, &["submodule/whisper.cpp/"]);
    CheckResult {
        key: "whisper_bin".into(),
        status: if stale {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        data: json!({ "path": path.display().to_string(), "msg": if stale { "可能过时" } else { "已编译" } }),
        required: false,
    }
}

pub fn check_demucs_ggml_bin() -> CheckResult {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let path = repo_root()
        .join("submodule")
        .join("demucs.cpp")
        .join("build")
        .join(format!("demucs_mt.cpp.main{ext}"));
    if !path.exists() {
        return CheckResult {
            key: "demucs_ggml_bin".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "demucs.cpp ggml 未编译" }),
            required: false,
        };
    }
    let stale = is_stale(&path, &["submodule/demucs.cpp/cli-apps/"]);
    CheckResult {
        key: "demucs_ggml_bin".into(),
        status: if stale {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        data: json!({ "path": path.display().to_string(), "msg": if stale { "可能过时" } else { "已编译" } }),
        required: false,
    }
}

/// 扫描 `target/{release,debug}` 下以 `prefix` 开头的二进制 (排除 `.d`)。
fn scan_release_bins(prefix: &str) -> Vec<PathBuf> {
    let dir = repo_root().join("target");
    let mut out = Vec::new();
    for profile in ["release", "debug"] {
        let p = dir.join(profile);
        if !p.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && !name.ends_with(".d") {
                out.push(e.path());
            }
        }
    }
    out
}

/// 检查 burn 系二进制。
///
/// `required` 为「本次配置实际需要的后端后缀」(如 "tch" / "cuda"): 仅当它缺失才判 Fail
/// 且给出精确缺失信息; 其余变体缺失仅作提示。None 时保持旧行为 (全变体任一缺失即 Fail)。
fn check_burn_bins(
    key: &str,
    prefix: &str,
    expected: &[&str],
    watch: &[&str],
    required: Option<&str>,
) -> CheckResult {
    let files = scan_release_bins(prefix);
    let existing: std::collections::HashSet<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|e| !existing.contains(*e))
        .collect();

    if files.is_empty() {
        // 无任一产物: 若已知所需后端, 精确报缺失; 否则笼统报
        let precise = required.map(|r| format!("demucs-burn-{r}"));
        return CheckResult {
            key: key.to_string(),
            status: CheckStatus::Fail,
            data: json!({ "missing_bins": missing.join(", "), "msg": match precise {
                Some(b) => format!("未找到编译产物: 需要 {b}"),
                None => "未找到编译产物".to_string(),
            } }),
            required: false,
        };
    }

    let latest = get_latest_source(watch);
    let mut stale_bins = Vec::new();
    let mut fresh_bins = Vec::new();
    for f in &files {
        if let Some(t) = latest {
            if let Some(bt) = mtime_sec(f) {
                if bt < t {
                    stale_bins.push(f.file_name().unwrap().to_string_lossy().to_string());
                } else {
                    fresh_bins.push(f.file_name().unwrap().to_string_lossy().to_string());
                }
            }
        }
    }

    // 判定: 已知所需后端时, 仅该后端缺失/过时 → Fail/Warn; 其余变体缺失仅提示
    let (status, fail_msg) = match required {
        Some(req) => {
            let req_bin = format!("{prefix}{req}");
            let req_missing = !existing.contains(&req_bin);
            let req_stale = stale_bins.iter().any(|b| b == &req_bin);
            if req_missing {
                (
                    CheckStatus::Fail,
                    format!(
                        "缺失所需后端: {req_bin} (请先 cargo build -p demucs-burn --bin {req_bin})"
                    ),
                )
            } else if req_stale {
                (CheckStatus::Warn, format!("{req_bin} 可能过时"))
            } else {
                (
                    CheckStatus::Pass,
                    format!("{req_bin} 已编译且最新").to_string(),
                )
            }
        }
        None => {
            if !stale_bins.is_empty() || !missing.is_empty() {
                (CheckStatus::Warn, "部分缺失/过时".to_string())
            } else {
                (CheckStatus::Pass, "全部已编译且最新".to_string())
            }
        }
    };
    let binaries: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    CheckResult {
        key: key.to_string(),
        status,
        data: json!({
            "stale_bins": stale_bins.join(", "),
            "fresh_bins": fresh_bins.join(", "),
            "missing_bins": missing.join(", "),
            "binaries": binaries.join(", "),
            "msg": fail_msg
        }),
        required: false,
    }
}

pub fn check_voxcpm_burn_bin(required: Option<&str>) -> CheckResult {
    check_burn_bins(
        "voxcpm_burn_bin",
        "voxcpm-burn-",
        &[
            "voxcpm-burn-wgpu",
            "voxcpm-burn-cpu",
            "voxcpm-burn-vulkan",
            "voxcpm-burn-tch",
        ],
        &["packages/voxcpm-burn/", "submodule/voxcpm-rs/"],
        required,
    )
}

pub fn check_demucs_burn_bin(required: Option<&str>) -> CheckResult {
    check_burn_bins(
        "demucs_burn_bin",
        "demucs-burn-",
        &[
            "demucs-burn-wgpu",
            "demucs-burn-cpu",
            "demucs-burn-tch",
            "demucs-burn-rocm",
            "demucs-burn-cuda",
        ],
        &["packages/demucs_burn/", "submodule/demucs-rs/"],
        required,
    )
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

pub fn check_whisper_sherpa() -> CheckResult {
    let dir = whisper_model_dir().join("sherpa_onnx");
    let all = [
        dir.join("turbo-encoder.int8.onnx"),
        dir.join("turbo-decoder.int8.onnx"),
        dir.join("turbo-tokens.txt"),
    ]
    .iter()
    .all(|p| p.exists());
    CheckResult {
        key: "whisper_sherpa".into(),
        status: if all {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        data: json!({ "msg": if all { "sherpa 模型齐全" } else { "sherpa 模型缺失" } }),
        required: false,
    }
}

pub fn check_whisper_onnx() -> CheckResult {
    check_model(
        &whisper_model_dir().join("encoder_model.onnx"),
        "whisper_onnx",
        200.0,
    )
}

pub fn check_demucs_pth() -> CheckResult {
    check_model(
        &demucs_model_dir().join("htdemucs_ft.safetensors"),
        "demucs_pth",
        300.0,
    )
}

pub fn check_demucs_onnx() -> CheckResult {
    let stems = ["drums", "bass", "other", "vocals"];
    let mut missing = Vec::new();
    for s in stems {
        let p = demucs_model_dir().join(format!("htdemucs_ft_{s}_fp16weights.onnx"));
        if !p.exists() {
            missing.push(s.to_string());
        }
    }
    let found = stems.len() - missing.len();
    let status = if missing.is_empty() {
        CheckStatus::Pass
    } else if missing.len() == stems.len() {
        CheckStatus::Fail
    } else {
        CheckStatus::Warn
    };
    CheckResult {
        key: "demucs_onnx".into(),
        status,
        data: json!({ "found": found, "total": stems.len(), "missing": missing.join(", "), "msg": if missing.is_empty() { "demucs onnx 齐全".to_string() } else { format!("缺失 {} ({}/{})", missing.join(", "), found, stems.len()) } }),
        required: false,
    }
}

const DEMUCS_GGML_FILE: &str = "ggml-model-htdemucs-4s-f16.bin";

pub fn check_demucs_ggml() -> CheckResult {
    check_model(
        &demucs_model_dir().join(DEMUCS_GGML_FILE),
        "demucs_ggml",
        80.0,
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
// OCR C++ 二进制 (镜像 packages/env/items/ocr_cpp_bin.ts)
// ---------------------------------------------------------------------------

pub fn ocr_cpp_bin_path() -> PathBuf {
    let name = if cfg!(windows) {
        "subtitle_ocr_ort_cpp.exe"
    } else {
        "subtitle_ocr_ort_cpp"
    };
    let b = repo_root()
        .join("packages")
        .join("subtitle-ocr")
        .join("ort-cpp")
        .join("build");
    let candidates = [b.join("Release").join(name), b.join(name)];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates[1].clone()
}

pub fn check_ocr_cpp_bin() -> CheckResult {
    let path = ocr_cpp_bin_path();
    if !path.exists() {
        return CheckResult {
            key: "ocr_cpp_bin".into(),
            status: CheckStatus::Fail,
            data: json!({ "msg": "OCR C++ 二进制未编译" }),
            required: false,
        };
    }

    if cfg!(target_os = "linux") {
        let (ok, out, _) = try_exec("ldd", &[path.to_str().unwrap()], None);
        if ok && out.contains("not found") {
            return CheckResult {
                key: "ocr_cpp_bin".into(),
                status: CheckStatus::Warn,
                data: json!({ "path": path.display().to_string(), "runtime": "missing_libs", "msg": "动态库缺失 (ldd not found)" }),
                required: false,
            };
        }
    }

    // 源码 git 时间比较
    if let Some(bin_time) = mtime_sec(&path) {
        if let Some(src_time) = git_commit_time(&repo_root(), "packages/subtitle-ocr/ort-cpp/") {
            if src_time > bin_time {
                return CheckResult {
                    key: "ocr_cpp_bin".into(),
                    status: CheckStatus::Warn,
                    data: json!({ "path": path.display().to_string(), "msg": "可能过时" }),
                    required: false,
                };
            }
        }
    }

    CheckResult {
        key: "ocr_cpp_bin".into(),
        status: CheckStatus::Pass,
        data: json!({ "path": path.display().to_string(), "msg": "已编译" }),
        required: false,
    }
}

fn ensure_ocr_cpp_bin() -> CheckResult {
    let b = repo_root()
        .join("packages")
        .join("subtitle-ocr")
        .join("ort-cpp")
        .join("build");
    let s = repo_root()
        .join("packages")
        .join("subtitle-ocr")
        .join("ort-cpp");

    // rm -rf build
    let _ = std::fs::remove_dir_all(&b);

    // cmake -S <src> -B <build> (显式参数, 不字符串拼接; windows 加 vcpkg 工具链)
    let mut cfg = Command::new("cmake");
    cfg.arg("-S").arg(&s).arg("-B").arg(&b);
    if cfg!(windows) {
        let tc = repo_root()
            .join("submodule")
            .join("vcpkg")
            .join("scripts")
            .join("buildsystems")
            .join("vcpkg.cmake");
        cfg.arg(format!("-DCMAKE_TOOLCHAIN_FILE={}", tc.display()));
        cfg.arg("-DVCPKG_TARGET_TRIPLET=x64-windows");
    }
    let cfg_ok = cfg
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let build_ok = Command::new("cmake")
        .arg("--build")
        .arg(&b)
        .arg("--config")
        .arg("Release")
        .arg("--parallel")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let ok = cfg_ok && build_ok && ocr_cpp_bin_path().exists();
    CheckResult {
        key: "ocr_cpp_bin".into(),
        status: if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        data: json!({ "path": ocr_cpp_bin_path().display().to_string(), "msg": if ok { "编译成功" } else { "编译失败" } }),
        required: false,
    }
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
    m.insert("vcpkg", check_vcpkg);
    m.insert("vulkan", check_vulkan);
    m.insert("rocm", check_rocm);
    m.insert("cuda", check_cuda);
    m.insert("whisper_ggml", check_whisper_ggml);
    m.insert("whisper_vad", check_whisper_vad);
    m.insert("whisper_sherpa", check_whisper_sherpa);
    m.insert("whisper_onnx", check_whisper_onnx);
    m.insert("demucs_pth", check_demucs_pth);
    m.insert("demucs_onnx", check_demucs_onnx);
    m.insert("demucs_ggml", check_demucs_ggml);
    m.insert("voxcpm2_onnx", check_voxcpm2_onnx);
    m.insert("voxcpm2_pth", check_voxcpm2_pth);
    m.insert("submodule_whisper_cpp", check_submodule_whisper_cpp);
    m.insert("submodule_demucs_cpp", check_submodule_demucs_cpp);
    m.insert("submodule_demucs_rs", check_submodule_demucs_rs);
    m.insert("submodule_voxcpm_rs", check_submodule_voxcpm_rs);
    m.insert("whisper_bin", check_whisper_bin);
    m.insert("demucs_ggml_bin", check_demucs_ggml_bin);
    m.insert("voxcpm_burn_bin", || check_voxcpm_burn_bin(None));
    m.insert("demucs_burn_bin", || check_demucs_burn_bin(None));
    m.insert("ocr_cpp_bin", check_ocr_cpp_bin);
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
    m.insert("ocr_cpp_bin", ensure_ocr_cpp_bin);
    m
}
