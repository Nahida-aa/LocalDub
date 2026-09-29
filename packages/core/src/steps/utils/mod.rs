//! step 共享基础设施 (镜像 TS `packages/core/steps/utils/utils.ts` + `context.ts` 的 step/workflow 持久化)。
//!
//! 提供:
//! - [`now_iso`] — RFC3339 无毫秒时间戳
//! - [`set_step`] / [`set_workflow`] — 对 `state.json` 中 steps / workflow 的 upsert 合并
//! - 各 step 输出目录 helper ([`separate_dir`] / [`asr_dir`] / [`separate_after_dir`] ...)
//! - [`steps`] 模块: output_mode 阶段序列 ([`steps::get_steps`])

pub mod paths;
pub mod srt;
pub mod steps;

use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::Serialize;

use crate::r#const::lang::LangCode;
use crate::state::{read_ctx, write_ctx, WorkflowStep};
use subtitle::SubtitleSegment;

/// RFC3339 时间戳, 去毫秒 (镜像 TS `nowISO`, 形如 `2024-01-01T00:00:00Z`)。
pub fn now_iso() -> String {
    // chrono 默认输出含毫秒 (`.%3f`), 这里截断到秒并补 `Z`。
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 取 video_dir 的最后一段作为 workflow id (镜像 TS `getLastSegment` / `getTaskId`)。
pub fn video_id(video_dir: &str) -> Option<String> {
    Path::new(video_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// step / workflow 持久化 (镜像 TS context.ts 的 setStep / setTask)
// ---------------------------------------------------------------------------

/// 部分更新 [`crate::state::StepState`] 的字段 (对应 TS `Partial<WorkflowStep>`)。
#[derive(Default, Clone)]
pub struct StepPatch {
    pub status: Option<crate::state::StepStatus>,
    pub progress: Option<f64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_message: Option<String>,
    pub error_message: Option<String>,
}

impl StepPatch {
    /// 合并进已有的 [`crate::state::StepState`] (TS `{...existing, ...patch}`)。
    fn apply(self, mut base: crate::state::StepState) -> crate::state::StepState {
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
        if matches!(base.status, crate::state::StepStatus::Success) {
            base.error_message = None;
        }
        base
    }
}

/// 对 `<step>/state.json` 做 RMW 合并 (镜像 TS `setStep`)。
/// 只碰该步骤自己的文件——并行分支各自写各自的文件，互不竞争。
pub fn set_step(video_dir: &str, name: &str, patch: StepPatch) -> Result<(), String> {
    let mut st = crate::state::read_step_state(video_dir, name)?;
    st = patch.apply(st);
    crate::state::write_step_state(video_dir, name, &st)
}

/// 部分更新 [`crate::state::Workflow`] 的字段 (镜像 TS `setTask`)。
#[derive(Default, Clone)]
pub struct WorkflowPatch {
    pub status: Option<String>,
    pub current_step: Option<Option<String>>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}

impl WorkflowPatch {
    fn apply(self, mut t: crate::state::Workflow) -> crate::state::Workflow {
        if let Some(v) = self.status {
            t.status = v;
        }
        if let Some(v) = self.current_step {
            t.current_step = v;
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
        // 标记 success 时清空 error_message (镜像 TS)
        if t.status == "success" {
            t.error_message = None;
        }
        t
    }
}

/// 对 `init.json` 中 workflow 子树做 RMW 合并 (镜像 TS `setTask`)。
/// **编排器专用**（run 边界）——步骤不写 init.json。
pub fn set_workflow(video_dir: &str, patch: WorkflowPatch) -> Result<(), String> {
    let mut init = crate::state::read_init(video_dir)?;
    init.workflow = patch.apply(init.workflow);
    crate::state::write_init(video_dir, &init)
}

/// `set_step` 返回 `Result<(), String>`, 此 wrapper 转 `anyhow::Result` 以便 `?`。
pub fn set_step_anyhow(video_dir: &str, name: &str, patch: StepPatch) -> anyhow::Result<()> {
    set_step(video_dir, name, patch).map_err(anyhow::Error::msg)
}

/// `set_workflow` 返回 `Result<(), String>`, 此 wrapper 转 `anyhow::Result` 以便 `?`。
pub fn set_workflow_anyhow(video_dir: &str, patch: WorkflowPatch) -> anyhow::Result<()> {
    set_workflow(video_dir, patch).map_err(anyhow::Error::msg)
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
pub fn final_video_dir(output_mode: &str, subtitle_source: &str, no_translate: bool) -> String {
    let suffix = if subtitle_source == "asr_ocr" {
        "_asr_ocr"
    } else if subtitle_source == "sf_ocr" {
        "_sf_ocr"
    } else {
        ""
    };
    let ntl_suffix = if no_translate { "_ntl" } else { "" };
    let mode = if output_mode == "subtitle" {
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
pub fn dubbing_path(video_dir: &str) -> PathBuf {
    Path::new(video_dir)
        .join("mix_audio")
        .join("audio_dubbing.wav")
}

/// `mix_audio/timings.json` 路径 (镜像 TS `timings_filepath`)。
pub fn mix_audio_timings_path(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("mix_audio").join("timings.json")
}

/// 读取 `mix_audio/timings.json` (镜像 TS `read_timings`)。
pub fn read_timings(
    video_dir: &str,
) -> anyhow::Result<crate::steps::mix_audio::out::MixAudioOutputTimings> {
    let p = mix_audio_timings_path(video_dir);
    let raw = std::fs::read_to_string(&p)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}", p.display()))?;
    serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", p.display()))
}

// ---------------------------------------------------------------------------
// 输出目录 / 路径 helper (镜像 TS utils.ts 的 *_dir / *_path)
// ---------------------------------------------------------------------------

pub fn separate_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("separate")
}
pub fn asr_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("asr")
}
pub fn separate_after_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("separate_after")
}

/// separate 阶段人声 stem (target_3_vocals.wav)
pub fn vocals_path(video_dir: &str) -> PathBuf {
    crate::steps::utils::paths::separate::Target3VocalsPath::new(video_dir).into()
}
/// separate_after 阶段背景音乐 stem
pub fn bgm_path(video_dir: &str) -> PathBuf {
    separate_after_dir(video_dir).join("target_bgm.wav")
}
/// separate_after 阶段混音后的人声
pub fn mixed_vocals_path(video_dir: &str) -> PathBuf {
    separate_after_dir(video_dir).join("target_3_vocals_mixed.wav")
}
/// separate_after 阶段 gate 后的人声
pub fn gated_vocals_path(video_dir: &str) -> PathBuf {
    separate_after_dir(video_dir).join("target_3_vocals_gated.wav")
}

/// sf_ocr_pre 关键帧目录
pub fn sf_ocr_pre_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("sf_ocr_pre")
}
/// sf_ocr OCR 结果目录
pub fn sf_ocr_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("sf_ocr")
}
/// sf_ocr_fix 修正结果目录
pub fn sf_ocr_fix_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("sf_ocr_fix")
}

/// asr_ocr_pre 分割 ASR + 抽帧目录
pub fn asr_ocr_pre_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("asr_ocr_pre")
}
/// asr_ocr OCR 结果目录
pub fn asr_ocr_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("asr_ocr")
}
/// asr_ocr_fix 融合修正结果目录
pub fn asr_ocr_fix_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("asr_ocr_fix")
}
/// asr_fix 修正结果目录
pub fn asr_fix_dir(video_dir: &str) -> PathBuf {
    Path::new(video_dir).join("asr_fix")
}

/// 取 video_source_path (缺则报错, 与 TS `video_source_path` 一致)。
pub fn video_source_path(ctx: &crate::state::State) -> anyhow::Result<String> {
    ctx.init.video_source_path.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "video_source_path 未设置 (session {})",
            ctx.init.workflow.video_dir
        )
    })
}

/// 确保目录存在 (镜像 TS `ensureDir`)。
pub fn ensure_dir(path: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).map_err(|e| anyhow::anyhow!("创建目录 {} 失败: {}", path.display(), e))
}

/// 写**规范化字幕产物** `srt.json`（`{"result": {"segments": [SubtitleSegment]}}`）。
///
/// 这是下游消费者（`translate` / `split_audio` / `mix_video`）的**唯一字幕契约**：
/// 各 fix 步骤（`asr_fix` / `sf_ocr_fix` / `asr_ocr_fix`）收尾时写出——**llmFix
/// 与否由产出方消化**（先写各自带内部字段的详细文件，最后统一落 srt.json），
/// 消费者不再需要知道产出方内部有没有跑过 LLM 修正。
///
/// 段结构用 `subtitle` crate 的 [`SubtitleSegment`]（text/start_ms/end_ms，
/// 权威定义在 vision-lab）。
///
/// # fail-fast：时间轴必须 `start_ms < end_ms`
///
/// 坏数据在**产出方**就报错，而不是等下游去猜。为什么必须拦：
///
/// - `pad_segments` 的两条 padding 路径基准不同（start 用 `start_ms`、end 用
///   `end_ms` 量到下一段的 gap），段反向时后者会把 gap 算得虚大，产出
///   `split_start >= split_end` 的**反向边界**——错误被放大而非暴露
/// - `split_audio` 对反向边界只写 44 字节空 wav 占位（不崩），而 `tts` 读的是
///   `timings.json` 的段、不是这些 wav——**问题被掩盖到成片里**
///
/// **`start == end` 也算错**（零长段）：它同样会在下游变成空产物，且没有任何
/// 合法场景需要「零长度的字幕段」。
pub fn write_srt_json(path: &std::path::Path, segments: &[SubtitleSegment]) -> anyhow::Result<()> {
    if let Some((i, s)) = segments
        .iter()
        .enumerate()
        .find(|(_, s)| s.start_ms >= s.end_ms)
    {
        // 从路径反推产出方步骤名（`{video_dir}/{step}/srt.json`），
        // 让错误直接指向**哪个步骤**产出了坏数据。
        let step = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        return Err(anyhow::anyhow!(
            "{step}: 第 {} 段时间轴非法 (start_ms={} >= end_ms={}); \
             请检查该步骤的输入数据（段必须满足 start_ms < end_ms）",
            i + 1,
            s.start_ms,
            s.end_ms
        ));
    }

    let out = serde_json::json!({ "result": { "segments": segments } });
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(&out)
        .map_err(|e| anyhow::anyhow!("序列化 srt.json 失败: {e}"))?;
    fs::write(path, json).map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", path.display(), e))?;
    Ok(())
}

/// 读 [`write_srt_json`] 产出的规范化字幕。
pub fn read_srt_json(path: &std::path::Path) -> anyhow::Result<Vec<SubtitleSegment>> {
    let raw = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", path.display()))?;
    serde_json::from_value(
        v.get("result")
            .and_then(|r| r.get("segments"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|e| anyhow::anyhow!("解析 {} 的 segments 失败: {e}", path.display()))
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

// ---------------------------------------------------------------------------
// 字幕/翻译/切分 路径 helper (镜像 TS steps/utils/utils.ts)
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

/// 翻译增量进度文件路径 `translate/{lang}.partial.json`。
///
/// 与最终产物 [`crate::steps::utils::paths::translate::LangPath`] 同一目录、
/// 同一语言前缀，只是多了 `.partial` 后缀。
pub fn translation_partial_path(video_dir: &str, lang: &str) -> PathBuf {
    Path::new(video_dir)
        .join("translate")
        .join(format!("{lang}.partial.json"))
}

/// split_audio 结果路径 `split_audio/split_audio.json` (padding 后时序)。
pub fn split_audio_path(video_dir: &str) -> PathBuf {
    Path::new(video_dir)
        .join("split_audio")
        .join("split_audio.json")
}

/// split_audio 意图时序路径 `split_audio/timings.json`。
pub fn split_audio_timings_path(video_dir: &str) -> PathBuf {
    Path::new(video_dir)
        .join("split_audio")
        .join("timings.json")
}

/// 读取翻译结果 → 请用 [`crate::steps::translate::out::TranslateResult::read`]。
///
/// 本函数已删：它返 `serde_json::Value`，逼得每个消费者再手写一遍字段解析
/// （`split_audio` 30 行、`mix_video` 一个 `extract_segs_from_value`），
/// 且每处 `unwrap_or(0)` 静默吞错。现在翻译产物是**带类型**读取的，解析只在
/// 一处。需要 ctx 版本时：`TranslateResult::read(&LangPath::from_state(ctx)?)`。

/// 解析目标语言。返回 `(srcLang, targetLang)`。
///
/// **规则**（按序）：
/// 1. `translate.enabled == false`（直通）→ **目标语言 = 源语言**。
///    直通时 dst 装的是原文，产物就该以原文语言命名；否则会出现
///    `translate/en.json` 里装中文这种名实不符，且下游按"目标语言"
///    选字体时（`default_font`：非 zh → Arial）会给中文字幕配 Arial。
/// 2. 否则 `input.workflow.targetLang`
/// 3. 否则 auto 推断（源 zh → en，其它 → zh）
///
/// 直通时**无条件用源语言**，即使配了 `targetLang` 也一样——「目标语言」在
/// 没有翻译的前提下不成立，此时它只是"以后打算翻译成什么"的意向，不是本次
/// 事实。本次事实由产物名与 `ctx.target_language()` 承载，必须是源语言。
///
/// 目标语言是任务级概念（一个任务只有一个翻译阶段），配置入口统一为
/// `input.workflow.targetLang`；旧 `steps.translate.targetLang` 已移除。
///
/// **本函数是纯函数**（无副作用）。落盘由
/// [`persist_target_language`] 完成，且只由 translate 调一次。
pub fn resolve_language(ctx: &crate::state::State) -> anyhow::Result<(String, String)> {
    let src = crate::state::lang::SourceLang::from_state(ctx);
    let src_code = src.code().to_string();

    // ① 直通：目标 = 源。不查配置、不推断——没有翻译就没有"目标"这个概念。
    if !translate_enabled(ctx) {
        return Ok((src_code.clone(), src_code));
    }

    // ②③ 目标语言: input.workflow.targetLang > auto 推断 (源 zh -> en, 其它 -> zh)。
    // 目标必须是支持的翻译语言: 显式值经 `Language` 枚举校验 (非法回落 auto)。
    // 解析委托给 `TargetLang::from_state`，与各 step 共用同一套解析，避免各自内联。
    let target = crate::state::lang::TargetLang::from_state(ctx);
    Ok((src_code, target.code().to_string()))
}

/// 解析**源语言**（与目标语言无关，故独立成函数）。
///
/// 委托给 [`crate::state::lang::SourceLang::from_state`]——单一解析来源，
/// 各 step（`asr_fix` 的 LLM 提示词、`split_audio` 写 meta、`sf_ocr_fix` /
/// `asr_ocr_fix` 的 LLM 修正）拿到的必然一致。
///
/// 回退链：`ctx.asr_language()`（ASR 实测）> `input.workflow.sourceLang` > 兜底（未知/空串）。
///
/// **为什么纯 OCR / subtitle 路径依赖后两级**：那些路径没有 ASR，
/// `ctx.asr_language()` 是 `None`——必须回落到配置的 `sourceLang`，否则非中文任务
/// 被当成 zh，进而把目标语言错误推断成 en。
///
/// 源语言是开放事实（`LangCode`），不做枚举截断——列表外语言如实透传。
pub fn src_language(ctx: &crate::state::State) -> LangCode {
    crate::state::lang::SourceLang::from_state(ctx).into_inner()
}

/// `steps.translate.enabled`，缺省 `true`（与 `TranslateArgs` 的 serde 默认一致）。
///
/// 直读结构化字段而非构造 `TranslateArgs`：本函数在 `utils` 层，不该依赖 translate
/// 的 args 类型（那会形成 utils → steps::translate 的循环依赖）。
fn translate_enabled(ctx: &crate::state::State) -> bool {
    ctx.init.input.steps.translate.enabled
}

/// 把本次的目标语言**落盘**（state.json 的 `target_language`）。
///
/// **只能由 [`crate::steps::translate::step_translate`] 调用一次。**
/// 这是刻意的写入权限约束：
///
/// 此前这一步藏在 `resolve_language` 内部（「算出来变了就顺手写回」），于是
/// 一个**看起来纯读**的函数带着隐形写副作用。后果是产生顺序依赖——
/// `split_audio` 先调 `read_translation_result(ctx)`（读 ctx 里的旧值）**再**调
/// `resolve_language(ctx)`（顺带改写 state.json），读在写之前，读到的文件名与
/// 该步骤认定的语言就不是同一个。
///
/// 现在拆开：`resolve_language` 纯函数（可随便调、幂等、无副作用），写盘只此一处。
/// 「谁写的」在类型/调用点上可见，不再需要读者去猜某个函数有没有副作用。
///
/// 失败**返回 Err**（原先只 `warn`）——写不下去就意味着下游会读到错的语言，
/// 静默继续会让问题延后到「文件不存在」时才暴露，更难定位。
pub fn persist_target_language(video_dir: &str, lang: &str) -> anyhow::Result<()> {
    // 幂等：值相同免一次写盘（translate 测试钉住「resolve 调用无副作用」）。
    // 写入点 = translate 自己的 state 文件（单写者原则）。
    let mut st = crate::state::read_step_state(video_dir, "translate")
        .map_err(anyhow::Error::msg)?;
    let existing = st.facts.get("target_language").and_then(|v| v.as_str());
    if existing == Some(lang) {
        return Ok(());
    }
    if !st.facts.is_object() {
        st.facts = serde_json::Value::Object(serde_json::Map::new());
    }
    if let Some(obj) = st.facts.as_object_mut() {
        obj.insert(
            "target_language".to_string(),
            serde_json::Value::String(lang.to_string()),
        );
    }
    crate::state::write_step_state(video_dir, "translate", &st)
        .map_err(anyhow::Error::msg)?;
    tracing::info!(target: "output_mode", "target_language 已落盘: {lang}");
    Ok(())
}

/// 序列化辅助: 把任意可序列化值转成 pretty JSON 字符串 (测试 / 透传用)。
pub fn to_json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| e.to_string())
}

/// re-export 以便 step 模块直接 `use crate::steps::utils::*;`
pub use crate::state::StepStatus;

#[allow(unused_imports)]
use std::io::Write as _;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::read_ctx_from_value;
    use crate::workflows::args::StepName;
    use serde_json::json;

    fn ctx_at(video_dir: &str, input: serde_json::Value) -> State {
        let mut ctx = read_ctx_from_value(input).unwrap();
        ctx.init.workflow.video_dir = video_dir.to_string();
        ctx
    }

    #[test]
    fn now_iso_format() {
        let s = now_iso();
        assert!(s.ends_with('Z'));
        assert!(!s.contains('.'), "不应含毫秒: {s}");
        assert_eq!(s.len(), 20);
    }

    /// `write_srt_json` 对非法时间轴 fail-fast：`start_ms >= end_ms` → 报错。
    ///
    /// 为什么必须在**产出方**拦：坏数据流到下游只会被放大而非暴露——
    /// `pad_segments` 会把反向段算出反向边界，`split_audio` 对它只写 44 字节
    /// 空 wav 占位，而 `tts` 读的是 timings 的段（不是那些 wav）——问题一路
    /// 掩盖到成片。
    #[test]
    fn write_srt_json_rejects_invalid_timeline() {
        let dir = std::env::temp_dir()
            .join(format!("ld_srt_json_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);

        let seg = |start_ms: u32, end_ms: u32| subtitle::SubtitleSegment {
            text: "t".into(),
            start_ms,
            end_ms,
        };
        let p = std::path::Path::new(&dir).join("asr_fix").join("srt.json");

        // 反向 → 报错，且错误点明步骤名、段索引与具体数值
        let err =
            write_srt_json(&p, &[seg(1000, 2000), seg(5000, 3000)]).expect_err("反向段必须报错");
        let msg = err.to_string();
        assert!(msg.contains("asr_fix"), "应点明产出方步骤: {msg}");
        assert!(msg.contains('2'), "应点明段索引(第 2 段): {msg}");
        assert!(
            msg.contains("5000") && msg.contains("3000"),
            "应含具体数值: {msg}"
        );

        // 零长也算错（无合法场景需要零长度字幕段）
        write_srt_json(&p, &[seg(1000, 1000)]).expect_err("零长段必须报错");

        // 合法 → 通过，且确实落盘
        write_srt_json(&p, &[seg(0, 1), seg(1000, 2000)]).expect("合法段不该失败");
        assert!(p.exists(), "合法时应落盘");

        // 报错时**不该**留下产物（坏数据不能落盘）
        let _ = std::fs::remove_file(&p);
        let _ = write_srt_json(&p, &[seg(5000, 3000)]);
        assert!(!p.exists(), "非法数据不该落盘");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 直通（`translate.enabled=false`）→ **目标语言 = 源语言**。
    ///
    /// 没有翻译就没有"目标"这个概念：dst 装的是原文，产物就该以原文语言命名。
    /// 否则会出现 `translate/en.json` 里装中文这种名实不符，且 `default_font`
    /// 按"目标语言"选字体（非 zh → Arial）会给中文字幕配 Arial。
    #[test]
    fn passthrough_targets_source_language() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"sourceLang": "zh"},
                          "steps": {"translate": {"enabled": false}}}
            }),
        );
        let (src, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(src, "zh");
        assert_eq!(dst, "zh", "直通时目标应等于源（不是推断出的 en）");
    }

    /// 直通时**无条件**用源语言——即使显式配了 `targetLang`。
    ///
    /// 理由：「目标语言」在没有翻译的前提下不成立，配了也只是"以后打算翻译成
    /// 什么"的意向，不是本次事实。本次事实由产物名与 `ctx.target_language()`
    /// 承载，必须是源语言。
    #[test]
    fn passthrough_overrides_explicit_target_lang() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"sourceLang": "zh", "targetLang": "vi"},
                          "steps": {"translate": {"enabled": false}}}
            }),
        );
        let (_, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(dst, "zh", "直通时显式 targetLang 被源语言覆盖");
    }

    /// `enabled` 缺省为 true（与 `TranslateArgs` 的 serde 默认一致）→ 正常解析。
    ///
    /// 这条防的是「把缺省当成 false」——那会让所有没写 enabled 的任务突然
    /// 全部走直通，静默不翻译。
    #[test]
    fn missing_enabled_defaults_to_translating() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"sourceLang": "zh"}}
            }),
        );
        let (_, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(dst, "en", "缺省 enabled=true → 正常推断（源 zh → en）");
    }

    /// 开翻译时显式 `targetLang` 优先于推断。
    #[test]
    fn explicit_target_wins_when_translating() {
        let ctx = ctx_at(
            "/x",
            json!({
                "workflow": {"id":"t","video_dir":"/x","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"workflow": {"sourceLang": "zh", "targetLang": "vi"},
                          "steps": {"translate": {"enabled": true}}}
            }),
        );
        let (_, dst) = resolve_language(&ctx).unwrap();
        assert_eq!(dst, "vi");
    }

    #[test]
    fn set_step_upserts_and_merges() {
        let dir = std::env::temp_dir()
            .join(format!("ld_step_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "output_mode": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        // 第一次 set → 新建, 带 running + started_at
        set_step(
            &dir,
            "separate",
            StepPatch {
                status: Some(StepStatus::Running),
                started_at: Some("2024-01-01T00:00:01Z".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // 第二次 set → 合并 progress, 保留 started_at
        set_step(
            &dir,
            "separate",
            StepPatch {
                progress: Some(50.0),
                ..Default::default()
            },
        )
        .unwrap();

        let reread = read_ctx(&dir).unwrap();
        let st = reread.step(StepName::Separate);
        assert_eq!(st.status, StepStatus::Running);
        assert_eq!(st.progress, Some(50.0));
        assert_eq!(st.started_at.as_deref(), Some("2024-01-01T00:00:01Z"));

        // 标记 success 清空 error_message
        set_step(
            &dir,
            "separate",
            StepPatch {
                status: Some(StepStatus::Success),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("boom".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        let st = reread.step(StepName::Separate);
        assert_eq!(st.status, StepStatus::Success);
        assert!(st.error_message.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_workflow_patch() {
        let dir = std::env::temp_dir()
            .join(format!("ld_workflow_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);
        let ctx = ctx_at(
            &dir,
            json!({
                "workflow": {"id":"t","video_dir":dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {}, "output_mode": "dub"
            }),
        );
        write_ctx(&dir, &ctx).unwrap();

        set_workflow(
            &dir,
            WorkflowPatch {
                status: Some("success".into()),
                completed_at: Some("2024-01-01T00:00:09Z".into()),
                error_message: Some("stale".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let reread = read_ctx(&dir).unwrap();
        assert_eq!(reread.init.workflow.status, "success");
        assert!(reread.init.workflow.error_message.is_none());

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
        assert_eq!(video_id(d).as_deref(), Some("task123"));
    }
}
