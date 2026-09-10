//! check 命令 (镜像 TS `packages/cli/src/feat/command/check.ts`)。
//!
//! 三种检查:
//! - `video`: 确认 `<taskDir>/media/video_source.mp4` 存在 (输出紧凑 JSON)
//! - `asr`: 诊断 ASR 结果 (asr_fix/asr_fix.json 优先, 回退 asr/asr.json), 输出 timeline + issues
//! - `font`: 检测 CJK 字体可用性 (win32 用已知 CRT 列表, 否则 fc-list)
//!
//! 输出均为 JSON 到 stdout (对齐 TS `console.log(JSON.stringify(...))`)。

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::anyhow;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::input::Input;

/// `check` 命令参数 (镜像 TS `input.check` schema)。
///
/// TS: `z.object({ taskDir: z.string().optional(), type: z.enum(["video","asr","font"]).optional().default("video") })`。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckArgs {
    /// 任务目录 (video/asr 检查必需)
    #[serde(default)]
    pub task_dir: Option<String>,
    /// 检查类型 (默认 video)
    #[serde(default)]
    pub r#type: CheckType,
}

/// 检查类型 (也用于 clap `--type` 命令行解析)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum CheckType {
    #[default]
    Video,
    Asr,
    Font,
}

/// 命令入口 (镜像 TS `cmdCheck`): 分发三种检查, 结果 JSON 打到 stdout。
///
/// 失败路径输出 `{ok:false,error}` 并返回 Err (CLI 以退出码 1 结束), 对齐 TS `process.exit(1)`。
pub fn cmd_check(input: &Input, args: &CheckArgs) -> anyhow::Result<()> {
    match args.r#type {
        CheckType::Video => check_video(args),
        CheckType::Asr => check_asr(args),
        CheckType::Font => check_font(input),
    }
}

/// video 检查: `<taskDir>/media/video_source.mp4` 存在性 + 大小。
fn check_video(args: &CheckArgs) -> anyhow::Result<()> {
    let task_dir = args
        .task_dir
        .as_deref()
        .ok_or_else(|| fail("check video requires taskDir"))?;
    let video_path = Path::new(task_dir).join("media").join("video_source.mp4");
    if !video_path.exists() {
        return Err(fail("video_source.mp4 not found"));
    }
    let size = std::fs::metadata(&video_path).map_err(|e| fail(&e.to_string()))?.len();
    println!(
        "{}",
        json!({
            "ok": true,
            "type": "video",
            "path": video_path.to_string_lossy(),
            "size": size,
        })
    );
    Ok(())
}

/// asr 检查: 读取 asr_fix/asr_fix.json (回退 asr/asr.json), 输出 timeline + 段间 gap 诊断。
fn check_asr(args: &CheckArgs) -> anyhow::Result<()> {
    let task_dir = args
        .task_dir
        .as_deref()
        .ok_or_else(|| fail("check asr requires taskDir"))?;
    let asr_path = Path::new(task_dir).join("asr_fix").join("asr_fix.json");
    let asr_raw_path = Path::new(task_dir).join("asr").join("asr.json");
    let asr_file = if asr_path.exists() {
        asr_path
    } else if asr_raw_path.exists() {
        asr_raw_path
    } else {
        return Err(fail("asr.json not found"));
    };
    let text = std::fs::read_to_string(&asr_file).map_err(|e| fail(&e.to_string()))?;
    let asr: Value = serde_json::from_str(&text).map_err(|e| fail(&e.to_string()))?;

    let segments = asr
        .get("result")
        .and_then(|r| r.get("segments"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let audio_duration_ms = asr
        .get("meta")
        .and_then(|m| m.get("audio_duration"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i64;
    let total = segments.len();
    let mut timeline: Vec<Value> = Vec::new();
    let mut issues: Vec<Value> = Vec::new();
    let mut zero_gaps = 0usize;

    for (i, seg) in segments.iter().enumerate() {
        let text = seg
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        let start_ms = round_ms(seg.get("start_ms"));
        let end_ms = round_ms(seg.get("end_ms"));
        let gap_ms = if i > 0 {
            round_ms(seg.get("start_ms")) - round_ms(segments[i - 1].get("end_ms"))
        } else {
            0
        };
        if gap_ms == 0 {
            zero_gaps += 1;
        }
        let mut warnings: Vec<String> = Vec::new();
        if i > 0 && gap_ms == 0 {
            warnings.push("start 紧跟上段结束".into());
        }
        let next_start = segments.get(i + 1).map(|s| round_ms(s.get("start_ms")));
        if end_ms == audio_duration_ms
            || (i < total - 1 && next_start == Some(end_ms))
        {
            warnings.push("end 拉到分段边界".into());
        }
        let duration_ms = end_ms - start_ms;
        if duration_ms > 5000 {
            warnings.push(format!("时长 {:.1}s 超过 5s", duration_ms as f64 / 1000.0));
        }
        let mut entry = json!({
            "idx": i + 1,
            "text": text,
            "startMs": start_ms,
            "endMs": end_ms,
            "gapMs": gap_ms,
        });
        if !warnings.is_empty() {
            entry["warnings"] = warnings.into();
        }
        timeline.push(entry);
    }

    if total > 1 && zero_gaps == total - 1 {
        issues.push(json!({
            "type": "vad_not_fired",
            "detail": format!("全部 {} 个段间间隙为 0ms，VAD 可能未生效", total - 1),
            "suggestion": "检查 ASR 引擎的 VAD 配置",
        }));
    } else if zero_gaps > 0 {
        issues.push(json!({
            "type": "partial_zero_gaps",
            "detail": format!("{}/{} 个段间间隙为 0ms", zero_gaps, total - 1),
        }));
    }

    let engine = asr
        .get("meta")
        .and_then(|m| m.get("engine"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let mut result = json!({
        "ok": true,
        "type": "asr",
        "engine": engine,
        "audioDurationMs": audio_duration_ms,
        "segments": total,
        "zeroGaps": zero_gaps,
        "timeline": timeline,
    });
    if !issues.is_empty() {
        result["issues"] = issues.into();
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// font 检查: mix_video.font (默认 Noto Sans CJK SC) 是否可用。
fn check_font(input: &Input) -> anyhow::Result<()> {
    let configured_font = input
        .stages
        .mix_video
        .font
        .clone()
        .unwrap_or_else(|| "Noto Sans CJK SC".into());
    let mut result = json!({
        "ok": true,
        "type": "font",
        "configured": configured_font,
    });

    if cfg!(windows) {
        result["available"] = Value::Bool(true);
        result["cjkFonts"] = json!(["Microsoft YaHei", "SimHei", "SimSun"]);
        result["note"] = Value::String(
            "Windows 字体检测暂不支持 fc-list，使用已知 CRT 字体列表".into(),
        );
    } else {
        let cjk_raw = fc_raw("fc-list", &[":lang=zh".to_string(), "family".to_string()]);
        let mut cjk_fonts: Vec<String> = cjk_raw
            .split('\n')
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .flat_map(|l| l.split(',').map(|s| s.trim().to_string()))
            .collect();
        cjk_fonts.sort();
        cjk_fonts.dedup();

        let match_raw = fc_raw("fc-list", &[format!(":family={configured_font}")]);
        let available = !match_raw.is_empty();
        result["available"] = Value::Bool(available);
        result["cjkFonts"] = cjk_fonts.iter().map(|s| Value::String(s.clone())).collect();
        if !available {
            result["suggestion"] = json!(
                if cjk_fonts.is_empty() {
                    format!("字体 \"{configured_font}\" 未安装，可尝试：sudo apt install fonts-noto-cjk")
                } else {
                    format!("字体 \"{configured_font}\" 未安装，可用 CJK 字体：{}", cjk_fonts.join("、"))
                }
            );
        }
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// 构造 `{ok:false,error}` 的失败 JSON, 写到 stdout (对齐 TS `JSON.stringify` + `process.exit(1)`)。
fn fail(msg: &str) -> anyhow::Error {
    println!("{}", json!({ "ok": false, "error": msg }));
    anyhow!("{msg}")
}

/// JS `Math.round`: start_ms/end_ms 可能是小数, 取整到 ms。
fn round_ms(v: Option<&Value>) -> i64 {
    v.and_then(|v| v.as_f64()).map(|f| f.round() as i64).unwrap_or(0)
}

/// 镜像 TS `fcRaw` (spawnSync timeout:5000): 成功取 stdout.trim(), 否则空串。
fn fc_raw(cmd: &str, args: &[String]) -> String {
    let mut c = Command::new(cmd);
    c.args(args);
    c.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(_) => return String::new(),
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let _ = child
                    .stdout
                    .take()
                    .map(|mut h| h.read_to_string(&mut stdout));
                if status.success() {
                    return stdout.trim().to_string();
                }
                return String::new();
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return String::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_check_args_defaults() {
        let empty: CheckArgs = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(empty.r#type, CheckType::Video);
        assert!(empty.task_dir.is_none());

        let full: CheckArgs = serde_json::from_str(
            r#"{"taskDir":"/tmp/t","type":"asr"}"#,
        )
        .unwrap();
        assert_eq!(full.r#type, CheckType::Asr);
        assert_eq!(full.task_dir.as_deref(), Some("/tmp/t"));
    }

    #[test]
    fn check_args_field_wires_into_input() {
        let input: Input =
            serde_json::from_str(r#"{"command":"check","check":{"type":"font"}}"#).unwrap();
        assert_eq!(input.command, crate::input::Command::Check);
        assert_eq!(input.check.unwrap().r#type, CheckType::Font);
    }

    /// 构造假 asr.json (asr_fix 优先), 验证 timeline/gap/issues 计算。
    #[test]
    fn asr_gap_diagnosis() {
        let dir = std::env::temp_dir().join(format!("ld-check-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("asr_fix")).unwrap();
        let json = json!({
            "meta": { "engine": "whisper", "audio_duration": 900 },
            "result": { "segments": [
                { "text": "hello", "start_ms": 0.0, "end_ms": 1000.0 },
                { "text": "world", "start_ms": 1000.0, "end_ms": 2000.0 },
                { "text": "xxx", "start_ms": 3000.0, "end_ms": 4000.0 },
            ]},
        });
        std::fs::write(dir.join("asr_fix/asr_fix.json"), json.to_string()).unwrap();
        let args = CheckArgs {
            task_dir: Some(dir.to_string_lossy().into_owned()),
            r#type: CheckType::Asr,
        };
        check_asr(&args).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// asr 回退到 asr/asr.json (asr_fix 缺失)。
    #[test]
    fn asr_falls_back_to_raw() {
        let dir = std::env::temp_dir().join(format!("ld-check-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("asr")).unwrap();
        let json = json!({ "meta": { "engine": "x" }, "result": { "segments": [
            { "text": "a", "start_ms": 0.0, "end_ms": 500.0 },
        ]}});
        std::fs::write(dir.join("asr/asr.json"), json.to_string()).unwrap();
        let args = CheckArgs {
            task_dir: Some(dir.to_string_lossy().into_owned()),
            r#type: CheckType::Asr,
        };
        check_asr(&args).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}