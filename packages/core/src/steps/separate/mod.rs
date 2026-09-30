//! separate: demucs 分离人声与背景声, 提升 tts-vc 的质量。
//!
//! 镜像 TS `packages/core/steps/separate/`。逻辑编排: 校验输入 → 选择 demucs-burn 后端
//! → spawn 二进制 → 流式解析 `(xx%)` 进度 → 标记 step 完成。

pub mod after;
pub mod after_args;
pub mod args;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::steps::utils::{
    now_iso, separate_dir, set_step, set_step_anyhow, StepPatch, StepStatus,
};
use crate::workflows::args::OutputMode;

pub use after::step_separate_after;
pub use after_args::SeparateAfterInput;
pub use args::{SeparateArgs, SeparateInput};

/// 根据 runtime + device 选择 demucs-burn 后端 (镜像 TS `separateBurn` 的 backend 派生)。
/// `burn-tch` → tch (device 无关); `burn` → 按 device (Cpu/Mps→cpu, Cuda→cuda,
/// Vulkan→vulkan, Webgpu→wgpu)。须与 `cmd/env` 的 `demucs_backend_suffix` 保持一致。
fn backend_for(runtime: args::SeparateRuntime, device: args::Device) -> &'static str {
    match runtime {
        args::SeparateRuntime::BurnTch => "tch",
        args::SeparateRuntime::Burn => match device {
            args::Device::Cpu | args::Device::Mps => "cpu",
            args::Device::Cuda => "cuda",
            args::Device::Vulkan => "vulkan",
            args::Device::Webgpu => "wgpu",
        },
    }
}

/// 定位 demucs-burn 二进制:
/// 1. `data/bin` (release 下载, vox-lab zip 平铺);
/// 2. `target/release`, 回退 `target/debug` (dev 下 cargo 产物)。
fn demucs_bin_path(backend: &str) -> Option<PathBuf> {
    let name = format!("demucs-burn-{backend}");
    let downloaded = config_rs::path::models::bin_dir().join(&name);
    if downloaded.exists() {
        return Some(downloaded);
    }

    None
}

/// 手工解析 stdout 中的 `(xx%)` 进度 (镜像 TS `/\(\s*(\d+(?:\.\d+)?)%\)/`), 无 regex 依赖。
fn parse_progress_pct(s: &str) -> Option<i32> {
    let open = s.find('(')?;
    let rest = &s[open + 1..];
    let close = rest.find(')')?;
    let inner = rest[..close].trim();
    let pct = inner.strip_suffix('%').unwrap_or(inner).trim();
    let val: f64 = pct.parse().ok()?;
    Some(val.clamp(0.0, 100.0) as i32)
}

/// 运行 demucs-burn 分离, 流式把 stdout 的 `(xx%)` 进度写入 step。
///
/// 镜像 TS `separateBurn` 的 spawn + 进度解析 (`/\((\s*\d+(?:\.\d+)?)%\)/`).
/// 定位 LibTorch 共享库目录 (tch 后端运行时必须)。
///
/// 优先 release 下载场景: vox-lab 的 demucs-burn-tch zip 把 libtorch_cpu.so 等平铺在
/// `bin_dir()` (data/bin), 与二进制同目录。回退源码构建场景:
/// `target/{release,debug}/build/torch-sys-*/out/libtorch/libtorch/lib` (镜像 TS
/// `wrapper.ts` 的 `findLibtorchPath`)。
///
/// 若找不到, 返回 None (调用方据此在错误信息中提示先构建/下载 tch 后端)。
fn find_libtorch_lib_dir() -> Option<PathBuf> {
    let bin = config_rs::path::models::bin_dir();
    if bin.join("libtorch_cpu.so").exists() {
        return Some(bin);
    }
    let repo = config_rs::root::repo_root();
    for profile in ["release", "debug"] {
        let build_dir = repo.join("target").join(profile).join("build");
        // 该 profile 无 build 目录 (如只编过 debug) → 跳过, 不因此中断整个查找
        let entries = match std::fs::read_dir(&build_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("torch-sys-") {
                continue;
            }
            let lib = entry
                .path()
                .join("out")
                .join("libtorch")
                .join("libtorch")
                .join("lib");
            if lib.join("libtorch_cpu.so").exists() {
                return Some(lib);
            }
        }
    }
    None
}

fn run_demucs(
    video_dir: &str,
    bin_path: &std::path::Path,
    audio_path: &str,
    sep_dir: &std::path::Path,
) -> anyhow::Result<()> {
    tracing::info!(target: "separate", "spawn {}", bin_path.display());

    // tch 后端运行时依赖 LibTorch 动态库 (libtorch_cpu.so 等), 必须注入 LD_LIBRARY_PATH,
    // 否则 loader 找不到库 → exit 127 (镜像 TS wrapper.ts 的 env.LD_LIBRARY_PATH 注入)。
    let mut cmd = Command::new(bin_path);
    cmd.arg(audio_path)
        .arg(sep_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if bin_path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("tch"))
    {
        match find_libtorch_lib_dir() {
            Some(lib) => {
                let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
                let combined = if existing.is_empty() {
                    lib.to_string_lossy().into_owned()
                } else {
                    format!("{}:{}", lib.to_string_lossy(), existing)
                };
                tracing::info!(target: "separate", "tch 后端注入 LD_LIBRARY_PATH={}", lib.display());
                cmd.env("LD_LIBRARY_PATH", combined);
            }
            None => {
                return Err(anyhow::anyhow!(
                    "tch 后端二进制需要 LibTorch 动态库, 但未找到 libtorch_cpu.so。\
                     请先下载 demucs-burn-tch release 资产 (data/bin) 或编译 tch 后端: \
                     cargo build -p demucs-burn --bin demucs-burn-tch --features tch"
                ));
            }
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn demucs-burn 失败: {}", e))?;

    // 终端进度条 (对齐 sf-cli/ocr 的 indicatif UX, 走 stderr)。
    // demucs 进度经 stdout pipe 读入, 这里解析并渲染到 cli 的 stderr;
    // 非 TTY (重定向/后台日志) 时 indicatif 自动隐藏, 仅更新 step.progress。
    let pb = indicatif::ProgressBar::new(100);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "[{elapsed_precise}] 分离 [{bar:30.cyan/blue}] {pos}/{len}% ({eta})",
        )
        .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
        .progress_chars("=> "),
    );

    let mut last_pct: i32 = -1;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("无法获取 demucs-burn stdout"))?;
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    use std::io::Read;
    // 逐块读以正确处理未换行进度行; 手工解析 `(xx%)` (避免引入 regex 依赖)
    let mut buf = [0u8; 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        line.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(nl) = line.find('\n') {
            let cur = line[..nl].to_string();
            line.drain(..=nl);
            if let Some(pct) = parse_progress_pct(&cur) {
                if pct != last_pct {
                    last_pct = pct;
                    pb.set_position(pct as u64);
                    let _ = set_step(
                        video_dir,
                        "separate",
                        StepPatch {
                            progress: Some(pct as f64),
                            last_message: Some(format!("Separating {pct}%")),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    // 处理最后一段 (无换行)
    if let Some(pct) = parse_progress_pct(&line) {
        pb.set_position(pct as u64);
        let _ = set_step(
            video_dir,
            "separate",
            StepPatch {
                progress: Some(pct as f64),
                ..Default::default()
            },
        );
    }
    pb.finish_with_message("分离完成");

    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("等待 demucs-burn 退出失败: {}", e))?;
    if !status.success() {
        // 捕获 stderr 以提供明确错误信息 (原先只报 exit code, 不清晰)
        let stderr = child
            .stderr
            .take()
            .map(|mut s| {
                let mut buf = String::new();
                let _ = s.read_to_string(&mut buf);
                buf
            })
            .unwrap_or_default();
        let code = status.code().unwrap_or(-1);
        return Err(anyhow::anyhow!(
            "demucs-burn 失败 (exit {code})\n--- stderr ---\n{stderr}"
        ));
    }
    Ok(())
}

/// 入口 (镜像 TS `stepSeparate`)。
///
/// **单参**：本步的全部依赖已在 [`SeparateInput`] 里列清（身份 + 上游音频 + 本次
/// 参数 + 决策用的 output_mode），不再接一个"什么都装"的组合态。
pub fn step_separate(input: &SeparateInput) -> anyhow::Result<()> {
    let video_dir = input.video_dir.clone();
    tracing::info!(target: "separate", "start");

    let cfg = &input.args;

    // subtitle 模式且未配置 always → 跳过分离
    if input.output_mode == OutputMode::Subtitle && !cfg.always {
        tracing::info!(target: "separate", "Skipped (subtitle pipeline, set separate.always=true to force)");
        set_step_anyhow(
            &video_dir,
            "separate",
            StepPatch {
                status: Some(StepStatus::Success),
                completed_at: Some(now_iso()),
                progress: Some(100.0),
                last_message: Some("Skipped (subtitle pipeline)".into()),
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    set_step_anyhow(
        &video_dir,
        "separate",
        StepPatch {
            last_message: Some("Separating audio...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let audio_path = input
        .audio_source_path
        .as_path()
        .to_string_lossy()
        .into_owned();
    if !std::path::Path::new(&audio_path).exists() {
        return Err(anyhow::anyhow!("audio_source.wav not found"));
    }

    let backend = backend_for(cfg.runtime, cfg.device);
    let bin_path = match demucs_bin_path(backend) {
        Some(p) => p,
        None => {
            // 已发布后端 (tch/wgpu): 经 env ensure 走 release 下载; 其余后端无发布资产
            // (cpu/cuda/vulkan 源码构建路径已随 demucs_burn 迁移而移除)。
            match backend {
                "tch" => {
                    let p = crate::cmd::env::ensure_bin("demucs_burn_tch_bin")?;
                    if let Some(d) = p.parent().filter(|d| d.is_dir()) {
                        tracing::info!(target: "separate", "demucs-burn-tch 经 release 下载: {}", d.display());
                    }
                    p
                }
                "wgpu" => crate::cmd::env::ensure_bin("demucs_burn_wgpu_bin")?,
                other => {
                    return Err(anyhow::anyhow!(
                        "demucs-burn 后端 {other} 暂无发布资产。请切换 separate.runtime/device 到 \
                         burn-tch (tch) 或 burn+webgpu (wgpu)。"
                    ));
                }
            }
        }
    };

    // 模型存在性检查 (镜像 TS 的 modelPath 校验)
    let model_path = config_rs::path::models::demucs_model_dir().join("htdemucs_ft.safetensors");
    if !model_path.exists() {
        return Err(anyhow::anyhow!(
            "Model not cached at {}. Run demucs-burn-{} first to download it.",
            model_path.display(),
            backend
        ));
    }

    let sep_dir = separate_dir(&video_dir);
    std::fs::create_dir_all(&sep_dir)
        .map_err(|e| anyhow::anyhow!("创建 separate 目录失败: {}", e))?;

    tracing::info!(target: "separate",
        "runtime={} device={:?} binary={}",
        backend,
        cfg.device,
        bin_path.display()
    );

    let t0 = std::time::Instant::now();
    run_demucs(&video_dir, &bin_path, &audio_path, &sep_dir)?;
    let elapsed = t0.elapsed();
    tracing::info!(target: "separate", "Processed in {:.1}s", elapsed.as_secs_f64());

    // 校验 stem 产物
    for (i, name) in ["drums", "bass", "other", "vocals"].iter().enumerate() {
        let p = sep_dir.join(format!("target_{i}_{name}.wav"));
        if !p.exists() {
            tracing::info!(target: "separate", "WARN: {} not found", p.display());
        }
    }

    set_step_anyhow(
        &video_dir,
        "separate",
        StepPatch {
            status: Some(StepStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Separated".into()),
            ..Default::default()
        },
    )?;
    tracing::info!(target: "separate", "done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_progress_pct;

    #[test]
    fn parse_progress_variants() {
        assert_eq!(parse_progress_pct("(0%)"), Some(0));
        assert_eq!(parse_progress_pct("(50%)"), Some(50));
        assert_eq!(parse_progress_pct("(100%)"), Some(100));
        assert_eq!(parse_progress_pct("some text ( 73% ) more"), Some(73));
        assert_eq!(parse_progress_pct("(12.5%)"), Some(12)); // floor via f64→i32
        assert_eq!(parse_progress_pct("(150%)"), Some(100)); // clamp
        assert_eq!(parse_progress_pct("no parens"), None);
        assert_eq!(parse_progress_pct("(abc%)"), None);
    }
    use super::*;
    use serde_json::json;

    /// 从扁平夹具 JSON 取 `input` 子树（`SeparateArgs` 等只吃这一份）。
    fn input_with(input: serde_json::Value) -> crate::input::Input {
        serde_json::from_value(input.get("input").cloned().unwrap_or(json!({}))).unwrap()
    }

    #[test]
    fn field_defaults_when_absent() {
        let input = input_with(json!({"input": {"steps": {"separate": {}}}}));
        let cfg = SeparateArgs::from_input(&input);
        assert_eq!(cfg.runtime, args::SeparateRuntime::BurnTch);
        assert_eq!(cfg.device, args::Device::Cuda);
        assert!(!cfg.always);
        assert!(cfg.stems.is_empty());
    }

    #[test]
    fn from_input_parses_camel_case_fields() {
        let input = input_with(json!({
            "input": {"steps": {"separate": {
                "runtime": "burn-tch",
                "device": "mps",
                "always": true,
                "stems": ["drums", "vocals"]
            }}}
        }));
        let cfg = SeparateArgs::from_input(&input);
        assert_eq!(cfg.device, args::Device::Mps);
        assert!(cfg.always);
        assert_eq!(cfg.stems, vec![args::Stem::Drums, args::Stem::Vocals]);
    }

    #[test]
    fn backend_selection() {
        assert_eq!(
            backend_for(args::SeparateRuntime::Burn, args::Device::Webgpu),
            "wgpu"
        );
        assert_eq!(
            backend_for(args::SeparateRuntime::Burn, args::Device::Cuda),
            "cuda"
        );
        assert_eq!(
            backend_for(args::SeparateRuntime::BurnTch, args::Device::Cpu),
            "tch"
        );
    }

    #[test]
    fn subtitle_skips_when_not_always() {
        // subtitle + !always → skip 分支（不看磁盘上的音频是否存在）
        let dir = std::env::temp_dir()
            .join(format!("ld_sep_skip_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let input = crate::state::write_fixture(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"outputMode": "subtitle"},
                          "steps": {"separate": {"always": false}}},
                "output_mode": "subtitle"
            }),
        );
        let res = step_separate(&SeparateInput::new(&dir, &input).unwrap());
        assert!(res.is_ok(), "subtitle skip 不应失败: {:?}", res.err());
        let st = crate::state::read_step_state(&dir, "separate").unwrap();
        assert_eq!(st.status, StepStatus::Success);
        assert_eq!(
            st.last_message.as_deref(),
            Some("Skipped (subtitle pipeline)")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
