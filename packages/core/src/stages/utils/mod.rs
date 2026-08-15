//! stage 共享基础设施 (镜像 TS `packages/core/stages/utils/utils.ts` + `context.ts` 的 stage/task 持久化)。
//!
//! 提供:
//! - [`now_iso`] — RFC3339 无毫秒时间戳
//! - [`set_stage`] / [`set_task`] — 对 `ctx.json` 中 stages / task 的 upsert 合并
//! - 各 stage 输出目录 helper ([`separate_dir`] / [`asr_dir`] / [`separate_after_dir`] ...)
//! - [`emit_log`] — tracing + 追加 `<tid>.log`
//! - [`stages`] 模块: pipeline 阶段序列 ([`stages::get_stages`])

pub mod srt;
pub mod stages;

use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::Serialize;

use crate::context::{TaskStage, read_ctx, write_ctx};

/// RFC3339 时间戳, 去毫秒 (镜像 TS `nowISO`, 形如 `2024-01-01T00:00:00Z`)。
pub fn now_iso() -> String {
    // chrono 默认输出含毫秒 (`.%3f`), 这里截断到秒并补 `Z`。
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 取 task_dir 的最后一段作为 task id (镜像 TS `getLastSegment` / `getTaskId`)。
pub fn task_id(task_dir: &str) -> Option<String> {
    Path::new(task_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// stage / task 持久化 (镜像 TS context.ts 的 setStage / setTask)
// ---------------------------------------------------------------------------

/// 部分更新 [`TaskStage`] 的字段 (对应 TS `Partial<TaskStage>`)。
#[derive(Default, Clone)]
pub struct StagePatch {
    pub label: Option<String>,
    pub status: Option<crate::context::StageStatus>,
    pub progress: Option<f64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_message: Option<String>,
    pub error_message: Option<String>,
}

impl StagePatch {
    /// 合并进已有的 [`TaskStage`] (TS `{...existing, ...patch}`)。
    fn apply(self, mut base: TaskStage) -> TaskStage {
        if let Some(v) = self.label {
            base.label = v;
        }
        if let Some(v) = self.status {
            base.status = v;
        }
        if let Some(v) = self.progress {
            base.progress = Some(v);
        }
        if let Some(v) = self.started_at {
            base.started_at = Some(v);
        }
        if let Some(v) = self.completed_at {
            base.completed_at = Some(v);
        }
        if let Some(v) = self.last_message {
            base.last_message = Some(v);
        }
        if let Some(v) = self.error_message {
            base.error_message = Some(v);
        }
        // 标记 success 时清空 error_message (镜像 TS)
        if matches!(base.status, crate::context::StageStatus::Success) {
            base.error_message = None;
        }
        base
    }
}

/// 对 `ctx.json` 中指定 stage 做 upsert 合并 (镜像 TS `setStage`)。
pub fn set_stage(task_dir: &str, name: &str, patch: StagePatch) -> Result<(), String> {
    let mut ctx = read_ctx(task_dir)?;
    let stages = ctx.stages.get_or_insert_with(Vec::new);
    let idx = stages.iter().position(|s| s.name == name);
    let base = match idx {
        Some(i) => stages[i].clone(),
        None => TaskStage {
            name: name.to_string(),
            label: name.to_string(),
            ..Default::default()
        },
    };
    let updated = patch.apply(base);
    match idx {
        Some(i) => stages[i] = updated,
        None => stages.push(updated),
    }
    write_ctx(task_dir, &ctx)
}

/// 部分更新 [`crate::context::Task`] 的字段 (镜像 TS `setTask`)。
#[derive(Default, Clone)]
pub struct TaskPatch {
    pub status: Option<String>,
    pub current_stage: Option<Option<String>>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub final_video_path: Option<Option<String>>,
}

impl TaskPatch {
    fn apply(self, mut t: crate::context::Task) -> crate::context::Task {
        if let Some(v) = self.status {
            t.status = v;
        }
        if let Some(v) = self.current_stage {
            t.current_stage = v;
        }
        if let Some(v) = self.started_at {
            t.started_at = Some(v);
        }
        if let Some(v) = self.completed_at {
            t.completed_at = Some(v);
        }
        if let Some(v) = self.error_message {
            t.error_message = Some(v);
        }
        if let Some(v) = self.final_video_path {
            t.final_video_path = v;
        }
        // 标记 success 时清空 error_message (镜像 TS)
        if t.status == "success" {
            t.error_message = None;
        }
        t
    }
}

/// 对 `ctx.json` 中 task 做 upsert 合并 (镜像 TS `setTask`)。
pub fn set_task(task_dir: &str, patch: TaskPatch) -> Result<(), String> {
    let mut ctx = read_ctx(task_dir)?;
    ctx.task = patch.apply(ctx.task);
    write_ctx(task_dir, &ctx)
}

/// `set_stage` 返回 `Result<(), String>`, 此 wrapper 转 `anyhow::Result` 以便 `?`。
pub fn set_stage_anyhow(task_dir: &str, name: &str, patch: StagePatch) -> anyhow::Result<()> {
    set_stage(task_dir, name, patch).map_err(anyhow::Error::msg)
}

/// `set_task` 返回 `Result<(), String>`, 此 wrapper 转 `anyhow::Result` 以便 `?`。
pub fn set_task_anyhow(task_dir: &str, patch: TaskPatch) -> anyhow::Result<()> {
    set_task(task_dir, patch).map_err(anyhow::Error::msg)
}

// ---------------------------------------------------------------------------
// ffmpeg 执行 (镜像 TS utils.ts 的 ffmpeg())
// ---------------------------------------------------------------------------

/// 取 ffmpeg 二进制路径: 优先 `FFMPEG_PATH` 环境变量, 回退 PATH 中的 `ffmpeg`。
pub(crate) fn ffmpeg_bin() -> String {
    std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string())
}

/// 用 ffprobe 取媒体时长 (毫秒), 失败返回 0 (镜像 TS `probeDurationMs`)。
pub fn probe_duration_ms(path: &str) -> u64 {
    let bin = std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());
    let out = std::process::Command::new(&bin)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            path,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let secs: f64 = s.parse().unwrap_or(0.0);
            (secs * 1000.0).round() as u64
        }
        _ => 0,
    }
}

/// 用 ffprobe 取采样率 (Hz), 失败返回 48000 (镜像 TS `probeSampleRate`)。
pub fn probe_sample_rate(path: &str) -> u32 {
    let bin = std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());
    let out = std::process::Command::new(&bin)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=sample_rate",
            "-of",
            "csv=p=0",
            path,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            s.parse().unwrap_or(48000)
        }
        _ => 48000,
    }
}

/// 读取 split_audio 时序文件 `split_audio/timings.json` (镜像 TS `read_split_audio_timings`)。
pub fn read_split_audio_timings(
    ctx: &crate::context::TaskCtx,
) -> anyhow::Result<serde_json::Value> {
    let file = split_audio_timings_path(&ctx.task.task_dir);
    let raw = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", file.display(), e))?;
    serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", file.display(), e))
}

/// 读取 split_audio 结果文件 `split_audio/split_audio.json` (镜像 TS `read_split_audio`)。
pub fn read_split_audio(ctx: &crate::context::TaskCtx) -> anyhow::Result<serde_json::Value> {
    let file = split_audio_path(&ctx.task.task_dir);
    let raw = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", file.display(), e))?;
    serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析 {} 失败: {}", file.display(), e))
}

/// tts 结果文件路径 `tts/tts.json` (镜像 TS `tts_filepath`)。
pub fn tts_filepath(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("tts").join("tts.json")
}

/// 执行 ffmpeg (自动前置 `-y` 覆盖输出), 非零退出即报错 (含 stderr)。
/// 参数格式与 TS `ffmpeg(args)` 一致: 传入不含 `-y` 的参数字串切片。
pub fn ffmpeg(args: &[String]) -> anyhow::Result<()> {
    let bin = ffmpeg_bin();
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("-y");
    cmd.args(args);
    let out = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("无法执行 {bin} (PATH 中未找到? 需安装): {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!(
            "{bin} 退出码 {:?}: {}",
            out.status.code(),
            stderr
        ));
    }
    Ok(())
}

/// 带超时的 ffmpeg 执行 (毫秒)。用于 mix_video 等长编码 (镜像 TS `ffmpeg(args, 300_000)`)。
pub fn ffmpeg_timeout(args: &[String], timeout_ms: u64) -> anyhow::Result<()> {
    use std::process::Stdio;
    let bin = ffmpeg_bin();
    let mut child = std::process::Command::new(&bin)
        .arg("-y")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("无法执行 {bin} (PATH 中未找到? 需安装): {e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let mut stderr = String::new();
                    if let Some(mut out) = child.stderr.take() {
                        use std::io::Read;
                        let _ = out.read_to_string(&mut stderr);
                    }
                    return Err(anyhow::anyhow!(
                        "{bin} 退出码 {:?}: {}",
                        status.code(),
                        stderr
                    ));
                }
                return Ok(());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(anyhow::anyhow!("{bin} 超时 (>{timeout_ms}ms), 已终止"));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(anyhow::anyhow!("{bin} 等待失败: {e}")),
        }
    }
}
fn ffprobe_bin() -> String {
    std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string())
}

/// 用 ffprobe 取视频分辨率 (宽, 高), 失败返回 (0,0) (镜像 TS `probeVideoResolution`)。
pub fn probe_video_resolution(path: &str) -> (u32, u32) {
    let bin = ffprobe_bin();
    let out = std::process::Command::new(&bin)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
            path,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let mut it = s.split(',').map(|x| x.trim().parse::<u32>().unwrap_or(0));
            let w = it.next().unwrap_or(0);
            let h = it.next().unwrap_or(0);
            (w, h)
        }
        _ => (0, 0),
    }
}

/// 最终视频目录名 (镜像 TS `finalVideoDir`)。
pub fn final_video_dir(pipeline: &str, subtitle_source: &str, no_translate: bool) -> String {
    let suffix = if subtitle_source == "asr_ocr" {
        "_asr_ocr"
    } else if subtitle_source == "sf_ocr" {
        "_sf_ocr"
    } else {
        ""
    };
    let ntl_suffix = if no_translate { "_ntl" } else { "" };
    let mode = if pipeline == "subtitle" {
        "subtitle"
    } else {
        "dub"
    };
    format!("{mode}{suffix}{ntl_suffix}")
}

/// 默认字幕字体 (镜像 TS `defaultFont`)。仅 Linux 实现完整, 其他平台回退通用名。
pub fn default_font(dst_lang: &str) -> String {
    if dst_lang != "zh" {
        return "Arial".to_string();
    }
    // 与 TS 对齐: win32=Microsoft YaHei, darwin=PingFang SC, default=Noto Sans CJK SC
    match std::env::consts::OS {
        "windows" => "Microsoft YaHei".to_string(),
        "macos" => "PingFang SC".to_string(),
        _ => "Noto Sans CJK SC".to_string(),
    }
}

/// 最终视频音频混流后的中间音频路径 (dub 分支) helper。
pub fn dubbing_path(task_dir: &str) -> PathBuf {
    Path::new(task_dir)
        .join("mix_audio")
        .join("audio_dubbing.wav")
}

/// `mix_audio/timings.json` 路径 (镜像 TS `timings_filepath`)。
pub fn mix_audio_timings_path(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("mix_audio").join("timings.json")
}

/// 读取 `mix_audio/timings.json` (镜像 TS `read_timings`)。
pub fn read_timings(task_dir: &str) -> anyhow::Result<crate::stages::mix_audio::out::TimingsFile> {
    let p = mix_audio_timings_path(task_dir);
    let raw = std::fs::read_to_string(&p)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}", p.display()))?;
    serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", p.display()))
}

// ---------------------------------------------------------------------------
// 输出目录 / 路径 helper (镜像 TS utils.ts 的 *_dir / *_path)
// ---------------------------------------------------------------------------

pub fn separate_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("separate")
}
pub fn asr_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("asr")
}
pub fn separate_after_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("separate_after")
}

/// separate 阶段人声 stem (target_3_vocals.wav)
pub fn vocals_path(task_dir: &str) -> PathBuf {
    separate_dir(task_dir).join("target_3_vocals.wav")
}
/// separate_after 阶段背景音乐 stem
pub fn bgm_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_bgm.wav")
}
/// separate_after 阶段混音后的人声
pub fn mixed_vocals_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_3_vocals_mixed.wav")
}
/// separate_after 阶段 gate 后的人声
pub fn gated_vocals_path(task_dir: &str) -> PathBuf {
    separate_after_dir(task_dir).join("target_3_vocals_gated.wav")
}

/// sf_ocr_pre 关键帧目录
pub fn sf_ocr_pre_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("sf_ocr_pre")
}
/// sf_ocr OCR 结果目录
pub fn sf_ocr_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("sf_ocr")
}
/// sf_ocr_fix 修正结果目录
pub fn sf_ocr_fix_dir(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("sf_ocr_fix")
}

/// 取 video_source_path (缺则报错, 与 TS `video_source_path` 一致)。
pub fn video_source_path(ctx: &crate::context::TaskCtx) -> anyhow::Result<String> {
    ctx.video_source_path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("video_source_path 未设置 (session {})", ctx.task.task_dir))
}

/// 确保目录存在 (镜像 TS `ensureDir`)。
pub fn ensure_dir(path: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).map_err(|e| anyhow::anyhow!("创建目录 {} 失败: {}", path.display(), e))
}

/// 定位 workspace 构建的二进制: 优先 `target/release/<name>`, 回退 `target/debug/<name>`
/// (镜像 TS 的 `$REPO_ROOT/target/release/<bin>` 解析, 兼容 dev/debug 构建)。
pub fn find_release_bin(name: &str) -> Option<PathBuf> {
    let repo = config_rs::root::repo_root();
    for profile in ["release", "debug"] {
        let p = repo.join("target").join(profile).join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 构造 `cargo build -p <package> --bin <bin>` 命令 (复用当前仓库根 + cargo 可执行)。
/// `features` 非空时追加 `--features <a>,<b>` (各 burn 包按后端用不同 feature 编二进制)。
///
/// 注: 不在此处 `.output()`, 交由调用方决定同步/流式, 故只返回构造好的 [`Command`]。
fn cargo_build_cmd(package: &str, bin: &str, features: &[&str]) -> std::process::Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = std::process::Command::new(cargo);
    cmd.arg("build")
        .arg("-p")
        .arg(package)
        .arg("--bin")
        .arg(bin);
    if !features.is_empty() {
        let joined = features.join(",");
        cmd.arg("--features").arg(joined);
    }
    cmd
}

/// 缺失时自动编译 workspace 二进制, 返回编译产物路径。
///
/// 执行 `cargo build -p <package> --bin <bin> [--features ...]`, 成功后在
/// `target/release` / `target/debug` 中定位产物 (优先 release)。失败 (编译错误 / 产物缺失)
/// 时返回错误, 由调用方决定如何上报。
///
/// `features` 用于指定后端 feature (如 demucs-burn 的 `tch`/`wgpu`/`cuda`...), 因为各 burn 包
/// 用 `required-features` 把二进制绑定到对应 feature, 不显式 `--features` 会编译失败。
///
/// 用于「阶段内自动编译缺失二进制」(用户选项: 阶段内自动编译), 镜像 TS 侧手动
/// `cargo build` 的引导步骤, 减少「先去 build 再跑」的来回。
pub fn cargo_build_bin(package: &str, bin: &str, features: &[&str]) -> anyhow::Result<PathBuf> {
    let feat_str = if features.is_empty() {
        String::new()
    } else {
        format!(" --features {}", features.join(","))
    };
    let cmdline = format!("cargo build -p {package} --bin {bin}{feat_str}");
    tracing::info!("[auto-build] 未找到 {bin}, 执行: {cmdline}");
    let mut cmd = cargo_build_cmd(package, bin, features);
    let out = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("无法执行 `{cmdline}` (cargo 未安装/未找到?): {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!("编译 {bin} 失败 (`{cmdline}`):\n{stderr}"));
    }
    // 优先 release, 回退 debug (dev 构建)
    let repo = config_rs::root::repo_root();
    for profile in ["release", "debug"] {
        let p = repo.join("target").join(profile).join(bin);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(anyhow::anyhow!(
        "编译 {bin} 成功但未找到产物 (target/release 或 target/debug 下均无 {bin})"
    ))
}

// ---------------------------------------------------------------------------
// 字幕/翻译/切分 路径 helper (镜像 TS stages/utils/utils.ts)
// ---------------------------------------------------------------------------

/// 语言码 -> 展示名 (镜像 TS `LANG_NAMES`)。未命中也返回原码。
pub fn lang_name(code: &str) -> String {
    let m: &[(&str, &str)] = &[
        ("en", "English"),
        ("zh", "Chinese"),
        ("vi", "Vietnamese"),
        ("ja", "Japanese"),
        ("ko", "Korean"),
        ("fr", "French"),
        ("de", "German"),
        ("es", "Spanish"),
        ("pt", "Portuguese"),
        ("ru", "Russian"),
        ("ar", "Arabic"),
        ("hi", "Hindi"),
        ("th", "Thai"),
        ("id", "Indonesian"),
        ("ms", "Malay"),
        ("tl", "Tagalog"),
        ("my", "Burmese"),
        ("km", "Khmer"),
        ("lo", "Lao"),
        ("mn", "Mongolian"),
        ("ne", "Nepali"),
        ("ur", "Urdu"),
        ("bn", "Bengali"),
    ];
    m.iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| code.to_string())
}

/// 解析 subtitleSource (缺省 "asr")
fn subtitle_source(input: &serde_json::Value) -> String {
    input
        .get("task")
        .and_then(|v| v.get("subtitleSource"))
        .and_then(|v| v.as_str())
        .unwrap_or("asr")
        .to_string()
}

/// 权威字幕文件路径 (asr_fix / sf_ocr_fix / asr_ocr_fix 按 subtitleSource 决定)。
/// 镜像 TS `subtitleFilePath`。
pub fn subtitle_file_path(ctx: &crate::context::TaskCtx) -> String {
    let src = subtitle_source(&ctx.input);
    if src == "sf_ocr" {
        let llm_fix = ctx
            .input
            .get("stages")
            .and_then(|v| v.get("sf_ocr_fix"))
            .and_then(|v| v.get("llmFix"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let filename = if llm_fix {
            "segment_filter_llm_fix.json"
        } else {
            "segment_filter.json"
        };
        return sf_ocr_fix_dir(&ctx.task.task_dir)
            .join(filename)
            .to_string_lossy()
            .into_owned();
    }
    if src == "asr_ocr" {
        let llm_fix = ctx
            .input
            .get("stages")
            .and_then(|v| v.get("asr_ocr_fix"))
            .and_then(|v| v.get("llmFix"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let filename = if llm_fix {
            "asr_ocr_fused_llm_fix.json"
        } else {
            "asr_ocr_fused.json"
        };
        return Path::new(&ctx.task.task_dir)
            .join("asr_ocr_fix")
            .join(filename)
            .to_string_lossy()
            .into_owned();
    }
    Path::new(&ctx.task.task_dir)
        .join("asr_fix")
        .join("asr_fix.json")
        .to_string_lossy()
        .into_owned()
}

/// 翻译文件路径 `translate/translation.{lang}.json`。
pub fn translation_file_path(task_dir: &str, lang: &str) -> PathBuf {
    Path::new(task_dir)
        .join("translate")
        .join(format!("translation.{lang}.json"))
}

/// split_audio 结果路径 `split_audio/split_audio.json` (padding 后时序)。
pub fn split_audio_path(task_dir: &str) -> PathBuf {
    Path::new(task_dir)
        .join("split_audio")
        .join("split_audio.json")
}

/// split_audio 意图时序路径 `split_audio/timings.json`。
pub fn split_audio_timings_path(task_dir: &str) -> PathBuf {
    Path::new(task_dir).join("split_audio").join("timings.json")
}

/// 读取翻译结果 (镜像 TS `readTranslationResult`), 须已解析 target_language。
pub fn read_translation_result(ctx: &crate::context::TaskCtx) -> anyhow::Result<serde_json::Value> {
    let lang = ctx
        .target_language
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ctx.target_language 未设置, 无法读取翻译结果"))?;
    let file = translation_file_path(&ctx.task.task_dir, &lang);
    let raw = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读取翻译文件 {} 失败: {}", file.display(), e))?;
    serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("解析翻译文件 {} 失败: {}", file.display(), e))
}

/// 解析目标语言: input > auto 推断 (源语言 zh -> en, 否则 any -> zh)。
/// 与 TS `resolveLanguage` 一致: 若解析出的目标语言与 ctx.target_language 不同, 写回
/// ctx.json 的 target_language (通过 [`set_task`])。返回 (srcLang, targetLang)。
pub fn resolve_language(ctx: &crate::context::TaskCtx) -> anyhow::Result<(String, String)> {
    let input_target = ctx
        .input
        .get("stages")
        .and_then(|v| v.get("translate"))
        .and_then(|v| v.get("targetLang"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let src_lang = ctx.asr_language.clone().unwrap_or_else(|| "zh".to_string());
    let existing_dst = ctx
        .target_language
        .clone()
        .unwrap_or_else(|| "zh".to_string());
    let resolved = input_target
        .or_else(|| {
            if src_lang == "zh" {
                Some("en".to_string())
            } else {
                Some("zh".to_string())
            }
        })
        .unwrap();
    if resolved != existing_dst {
        // 写回 ctx.target_language, 供后续翻译文件命名 / split_audio 读取 (best-effort:
        // ctx.json 不存在时仅告警, 不影响当前 stage 返回解析结果)
        if let Err(e) = write_target_language(&ctx.task.task_dir, &resolved) {
            emit_log(
                Some(&ctx.task.task_dir),
                &format!("[WARN] 写回 target_language 失败: {e}"),
            );
        }
    }
    Ok((src_lang, resolved))
}

/// 单独写回 ctx.json 的 target_language 字段 (resolve_language 内部用)。
fn write_target_language(task_dir: &str, lang: &str) -> Result<(), String> {
    let mut ctx = read_ctx(task_dir)?;
    ctx.target_language = Some(lang.to_string());
    write_ctx(task_dir, &ctx)
}

// ---------------------------------------------------------------------------
// 日志 (镜像 TS `emitLog`)
// ---------------------------------------------------------------------------

/// 写日志: tracing info + 追加到 `<task_dir>/<tid>.log`。
pub fn emit_log(task_dir: Option<&str>, line: &str) {
    tracing::info!("{line}");
    let Some(dir) = task_dir else { return };
    let Some(tid) = task_id(dir) else { return };
    let log_path = Path::new(dir).join(format!("{tid}.log"));
    let entry = format!("[{}] {}\n", now_iso(), line);
    // 追加失败不应中断流程
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = f.write_all(entry.as_bytes());
    }
}

/// 序列化辅助: 把任意可序列化值转成 pretty JSON 字符串 (测试 / 透传用)。
pub fn to_json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| e.to_string())
}

/// re-export 以便 stage 模块直接 `use crate::stages::utils::*;`
pub use crate::context::StageStatus;

#[allow(unused_imports)]
use std::io::Write as _;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{TaskCtx, read_ctx_from_value};
    use serde_json::json;

    fn ctx_at(task_dir: &str, input: serde_json::Value) -> TaskCtx {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.task.task_dir = task_dir.to_string();
        ctx
    }

    #[test]
    fn now_iso_format() {
        let s = now_iso();
        assert!(s.ends_with('Z'));
        assert!(!s.contains('.'), "不应含毫秒: {s}");
        assert_eq!(s.len(), 20);
    }

    #[test]
    fn set_stage_upserts_and_merges() {
        let dir = std::env::temp_dir()
            .join(format!("ld_stage_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "pipeline": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        // 第一次 set → 新建, 带 running + started_at
        set_stage(
            &dir,
            "separate",
            StagePatch {
                status: Some(StageStatus::Running),
                started_at: Some("2024-01-01T00:00:01Z".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // 第二次 set → 合并 progress, 保留 started_at
        set_stage(
            &dir,
            "separate",
            StagePatch {
                progress: Some(50.0),
                ..Default::default()
            },
        )
        .unwrap();

        let reread = read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].name, "separate");
        assert_eq!(st[0].status, StageStatus::Running);
        assert_eq!(st[0].progress, Some(50.0));
        assert_eq!(st[0].started_at.as_deref(), Some("2024-01-01T00:00:01Z"));

        // 标记 success 清空 error_message
        set_stage(
            &dir,
            "separate",
            StagePatch {
                status: Some(StageStatus::Success),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("boom".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        let st = reread.stages.unwrap();
        assert_eq!(st[0].status, StageStatus::Success);
        assert!(st[0].error_message.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_task_patch() {
        let dir = std::env::temp_dir()
            .join(format!("ld_task_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "task": {"id":"t","task_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "pipeline": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        set_task(
            &dir,
            TaskPatch {
                status: Some("success".into()),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("stale".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        assert_eq!(reread.task.status, "success");
        assert!(reread.task.error_message.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_helpers() {
        let d = "/work/task123";
        assert_eq!(separate_dir(d), Path::new("/work/task123/separate"));
        assert_eq!(
            vocals_path(d),
            Path::new("/work/task123/separate/target_3_vocals.wav")
        );
        assert_eq!(task_id(d).as_deref(), Some("task123"));
    }
}
