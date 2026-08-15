//! separate: demucs 分离人声与背景声, 提升 tts-vc 的质量。
//!
//! 镜像 TS `packages/core/stages/separate/`。逻辑编排: 校验输入 → 选择 demucs-burn 后端
//! → spawn 二进制 → 流式解析 `(xx%)` 进度 → 标记 stage 完成。

pub mod after;
pub mod args;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::context::TaskCtx;
use crate::stages::utils::{
    StagePatch, StageStatus, cargo_build_bin, emit_log, now_iso, separate_dir, set_stage,
    set_stage_anyhow,
};

pub use after::stage_separate_after;
pub use args::SeparateArgs;

/// 从 `ctx.input.stages.separate` 解析配置 (与 TS default 对齐)
pub fn read_args(ctx: &TaskCtx) -> SeparateArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("separate"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 根据 runtime + device 选择 demucs-burn 后端 (镜像 TS `separateBurn` 的 backend 派生)。
/// `burn-tch` → tch (device 无关); `burn` → 按 device (Cpu/Mps→cpu, Cuda→cuda,
/// Vulkan→vulkan, Webgpu→wgpu)。须与 `cmd/env` 的 `demucs_backend_suffix` 保持一致。
fn backend_for(runtime: args::Runtime, device: args::Device) -> &'static str {
    match runtime {
        args::Runtime::BurnTch => "tch",
        args::Runtime::Burn => match device {
            args::Device::Cpu | args::Device::Mps => "cpu",
            args::Device::Cuda => "cuda",
            args::Device::Vulkan => "vulkan",
            args::Device::Webgpu => "wgpu",
        },
    }
}

/// 定位 demucs-burn 二进制: 优先 `target/release`, 回退 `target/debug` (dev 下 cargo 产物)。
fn demucs_bin_path(backend: &str) -> Option<PathBuf> {
    let repo = config_rs::root::repo_root();
    let name = format!("demucs-burn-{backend}");
    for profile in ["release", "debug"] {
        let p = repo.join("target").join(profile).join(&name);
        if p.exists() {
            return Some(p);
        }
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

/// 运行 demucs-burn 分离, 流式把 stdout 的 `(xx%)` 进度写入 stage。
///
/// 镜像 TS `separateBurn` 的 spawn + 进度解析 (`/\((\s*\d+(?:\.\d+)?)%\)/`).
/// 定位 LibTorch 共享库目录 (tch 后端运行时必须): 在
/// `target/{release,debug}/build/torch-sys-*/out/libtorch/libtorch/lib` 下查找
/// `libtorch_cpu.so`。镜像 TS `wrapper.ts` 的 `findLibtorchPath` (release 优先)。
///
/// 若找不到, 返回 None (调用方据此在错误信息中提示先 build tch 后端)。
fn find_libtorch_lib_dir() -> Option<PathBuf> {
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
    task_dir: &str,
    bin_path: &std::path::Path,
    audio_path: &str,
    sep_dir: &std::path::Path,
) -> anyhow::Result<()> {
    emit_log(&format!("spawn {}", bin_path.display()));

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
                emit_log(&format!("tch 后端注入 LD_LIBRARY_PATH={}", lib.display()));
                cmd.env("LD_LIBRARY_PATH", combined);
            }
            None => {
                return Err(anyhow::anyhow!(
                    "tch 后端二进制需要 LibTorch 动态库, 但未找到 libtorch_cpu.so。\
                     请先编译 tch 后端: cargo build -p demucs-burn --bin demucs-burn-tch --features tch"
                ));
            }
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn demucs-burn 失败: {}", e))?;

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
                    let _ = set_stage(
                        task_dir,
                        "separate",
                        StagePatch {
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
        let _ = set_stage(
            task_dir,
            "separate",
            StagePatch {
                progress: Some(pct as f64),
                ..Default::default()
            },
        );
    }

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

/// 入口 (镜像 TS `stageSeparate`)。
pub fn stage_separate(ctx: &TaskCtx) -> anyhow::Result<()> {
    let task_dir = ctx.task.task_dir.clone();
    emit_log("separate: start");

    let cfg = read_args(ctx);

    // subtitle 模式且未配置 always → 跳过分离
    if ctx.pipeline == "subtitle" && !cfg.always {
        emit_log("Skipped (subtitle pipeline, set separate.always=true to force)");
        set_stage_anyhow(
            &task_dir,
            "separate",
            StagePatch {
                status: Some(StageStatus::Success),
                completed_at: Some(now_iso()),
                progress: Some(100.0),
                last_message: Some("Skipped (subtitle pipeline)".into()),
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    set_stage_anyhow(
        &task_dir,
        "separate",
        StagePatch {
            last_message: Some("Separating audio...".into()),
            progress: Some(0.0),
            ..Default::default()
        },
    )?;

    let video_path = ctx
        .video_source_path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("video_source_path 未设置"))?;
    if !std::path::Path::new(&video_path).exists() {
        return Err(anyhow::anyhow!("video_source.mp4 not found"));
    }
    let audio_path = ctx
        .audio_source_path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("audio_source_path 未设置"))?;
    if !std::path::Path::new(&audio_path).exists() {
        return Err(anyhow::anyhow!("audio_source.wav not found"));
    }

    let backend = backend_for(cfg.runtime, cfg.device);
    let bin_name = format!("demucs-burn-{backend}");
    let bin_path = match demucs_bin_path(backend) {
        Some(p) => p,
        None => {
            // 阶段内自动编译缺失二进制 (用户选项: 阶段内自动编译)
            emit_log(&format!("未找到 {bin_name}, 尝试自动编译..."));
            cargo_build_bin("demucs-burn", &bin_name, &[backend], false).map_err(|e| {
                anyhow::anyhow!(
                    "{e}\n若编译失败, 请手动执行: cargo build -p demucs-burn --bin {bin_name} --no-default-features --features {backend}"
                )
            })?
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

    let sep_dir = separate_dir(&task_dir);
    std::fs::create_dir_all(&sep_dir)
        .map_err(|e| anyhow::anyhow!("创建 separate 目录失败: {}", e))?;

    emit_log(&format!(
        "runtime={} device={:?} binary={}",
        backend,
        cfg.device,
        bin_path.display()
    ));

    let t0 = std::time::Instant::now();
    run_demucs(&task_dir, &bin_path, &audio_path, &sep_dir)?;
    let elapsed = t0.elapsed();
    emit_log(&format!("Processed in {:.1}s", elapsed.as_secs_f64()));

    // 校验 stem 产物
    for (i, name) in ["drums", "bass", "other", "vocals"].iter().enumerate() {
        let p = sep_dir.join(format!("target_{i}_{name}.wav"));
        if !p.exists() {
            emit_log(&format!("WARN: {} not found", p.display()));
        }
    }

    set_stage_anyhow(
        &task_dir,
        "separate",
        StagePatch {
            status: Some(StageStatus::Success),
            completed_at: Some(now_iso()),
            progress: Some(100.0),
            last_message: Some("Separated".into()),
            ..Default::default()
        },
    )?;
    emit_log("separate: done");
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
    use crate::context::read_ctx_from_value;
    use serde_json::json;

    fn ctx_with(input: serde_json::Value, pipeline: &str) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = "/tmp/ld_sep_test".into();
        ctx.task.id = "t".into();
        ctx.pipeline = pipeline.into();
        ctx.video_source_path = Some("/tmp/ld_sep_test/video_source.mp4".into());
        ctx.audio_source_path = Some("/tmp/ld_sep_test/audio_source.wav".into());
        ctx
    }

    #[test]
    fn field_defaults_when_absent() {
        let ctx = ctx_with(
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"separate": {}}}
            }),
            "dub",
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.runtime, args::Runtime::BurnTch);
        assert_eq!(cfg.device, args::Device::Cuda);
        assert!(!cfg.always);
        assert!(cfg.stems.is_empty());
    }

    #[test]
    fn read_args_parses_camel_case_fields() {
        let ctx = ctx_with(
            json!({
                "task": {"id":"t","task_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"separate": {
                    "runtime": "burn-tch",
                    "device": "mps",
                    "always": true,
                    "stems": ["drums", "vocals"]
                }}}
            }),
            "dub",
        );
        let cfg = read_args(&ctx);
        assert_eq!(cfg.device, args::Device::Mps);
        assert!(cfg.always);
        assert_eq!(cfg.stems, vec![args::Stem::Drums, args::Stem::Vocals]);
    }

    #[test]
    fn backend_selection() {
        assert_eq!(
            backend_for(args::Runtime::Burn, args::Device::Webgpu),
            "wgpu"
        );
        assert_eq!(backend_for(args::Runtime::Burn, args::Device::Cuda), "cuda");
        assert_eq!(
            backend_for(args::Runtime::BurnTch, args::Device::Cpu),
            "tch"
        );
    }

    #[test]
    fn subtitle_skips_when_not_always() {
        // 用一个 fake ctx.json, 验证 subtitle + !always 走 skip 分支
        let dir = std::env::temp_dir()
            .join(format!("ld_sep_skip_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let mut ctx = ctx_with(
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"stages": {"separate": {"always": false}}},
                "pipeline": "subtitle"
            }),
            "subtitle",
        );
        ctx.task.task_dir = dir.clone();
        ctx.pipeline = "subtitle".to_string();
        crate::context::write_ctx(&dir, &ctx).unwrap();
        let res = stage_separate(&ctx);
        assert!(res.is_ok(), "subtitle skip 不应失败: {:?}", res.err());
        let reread = crate::context::read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st[0].name, "separate");
        assert_eq!(st[0].status, StageStatus::Success);
        assert_eq!(
            st[0].last_message.as_deref(),
            Some("Skipped (subtitle pipeline)")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
