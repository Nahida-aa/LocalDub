//! SRT 写出 (镜像 TS `packages/core/utils/srt.ts` 的 `writeSrt` + `validateSrtContent`)。
//!
//! 仅实现「基础分支」: 每段输出一条 SRT (TS 的 `splitSubtitle` 多 fragment 切分逻辑未移植,
//! 因当前 Rust 流程的 segments 已是合适粒度, 与一致行为足够)。

use std::fs;
use std::path::Path;

use derive_more::{Deref, DerefMut, From, Into};
use serde::{Deserialize, Serialize};
use subtitle::SubtitleSegment;

use crate::utils::file_ops::{ensure_parent_dir, write_json_pretty};

/// 规范化字幕段集合（镜像 TS `SubtitleSegment[]`，内容类型）。
///
/// 与 `TranslateResult` / `SplitAudioTimingResult` 同款：解析只在 [`read`](Self::read)、
/// 写出只在 [`write`](Self::write) 一处，字段缺失 / 时间轴非法（含零长段）直接失败，
/// 而不是被静默吞。`read_srt_json` / `write_srt_json` 经由此类型，逻辑只在这一处。
///
/// `srt.json` 形状：`{"result": {"segments": [SubtitleSegment]}}`（外层 `result` 信封由
/// [`write`](Self::write) 手动套上，与旧 `write_srt_json` 同款落点）。
#[derive(Debug, Clone, Default, Deref, DerefMut, From, Into, Serialize, Deserialize)]
pub struct SubtitleSegments(pub Vec<SubtitleSegment>);

impl SubtitleSegments {
    /// 读取规范化字幕 `srt.json`（`{"result": {"segments": [...]}}`）。
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}", path.display()))?;
        let v: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", path.display()))?;
        let segments: Vec<SubtitleSegment> = serde_json::from_value(
            v.get("result")
                .and_then(|r| r.get("segments"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|e| anyhow::anyhow!("解析 {} 的 segments 失败: {e}", path.display()))?;
        Ok(Self(segments))
    }

    /// 写出规范化字幕 `srt.json`（`{"result": {"segments": [...]}}`）。
    ///
    /// fail-fast：时间轴必须 `start_ms < end_ms`（含零长段）；从路径反推产出方步骤名，
    /// 让错误直接指向哪个步骤产出了坏数据（与旧 `write_srt_json` 同款语义）。
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some((i, s)) = self
            .0
            .iter()
            .enumerate()
            .find(|(_, s)| s.start_ms >= s.end_ms)
        {
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
        ensure_parent_dir(path).map_err(|e| anyhow::anyhow!("创建目录失败: {e}"))?;
        write_json_pretty(path, &serde_json::json!({ "result": { "segments": self.0 } }))
    }

    /// 从 **SRT 文本文件**读入（[`write_srt`](Self::write_srt) 的逆操作）。
    ///
    /// 与 [`read`](Self::read) 的区别：那个读 `srt.json`
    /// （`{"result":{"segments":[...]}}` 信封），这个读可读的 `.srt` 文本。
    ///
    /// 结构不合法的块（不足 3 行、时间行不是 `HH:MM:SS,mmm --> HH:MM:SS,mmm`）
    /// **跳过**——与 `write_srt` 跳过空文本段对应；全部跳过就得到空集合，
    /// 由调用方判断是否算错（如 `generate_meta` 对空集合报错）。
    pub fn read_srt(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取 SRT {} 失败: {e}", path.display()))?;
        let mut blocks: Vec<Vec<&str>> = Vec::new();
        let mut current: Vec<&str> = Vec::new();
        for line in raw.split('\n') {
            if line.trim().is_empty() {
                if !current.is_empty() {
                    blocks.push(std::mem::take(&mut current));
                }
            } else {
                current.push(line);
            }
        }
        if !current.is_empty() {
            blocks.push(current);
        }

        let mut segs = Vec::new();
        for b in blocks {
            if b.len() < 3 {
                continue;
            }
            // b[1] = "HH:MM:SS,mmm --> HH:MM:SS,mmm"
            let times: Vec<&str> = b[1].split(" --> ").collect();
            if times.len() != 2 {
                continue;
            }
            segs.push(SubtitleSegment {
                text: b[2..].join("\n"),
                start_ms: parse_srt_time(times[0]),
                end_ms: parse_srt_time(times[1]),
            });
        }
        Ok(Self(segs))
    }

    /// 写出可读 SRT 文本文件（`.srt`，时间轴 `HH:MM:SS,mmm --> HH:MM:SS,mmm`）。
    ///
    /// 内容来自本集合的 `SubtitleSegment`（`text` / `start_ms` / `end_ms`）：
    /// `SubtitleSegment` 只有一个 `text`，没有原文/译文之分，也不带
    /// `actual_start` / `actual_end` 覆盖——「用哪种文本」在构造本集合时
    /// 就已定好（如 `MixAudioOutputTimings` / `SplitAudioTimingResult` 的
    /// `From` 转换取译文；translate 直通时译文即原文）。
    ///
    /// 空文本段会被跳过（不占序号）；写盘前经 [`validate_srt_content`] 预检，
    /// 避免交给 ffmpeg 时报含糊的 "Unable to open"。
    pub fn write_srt(&self, output: &Path) -> anyhow::Result<()> {
        let mut lines: Vec<String> = Vec::new();
        let mut idx: u32 = 1;
        for seg in &self.0 {
            if seg.text.trim().is_empty() {
                continue;
            }
            lines.push(idx.to_string());
            lines.push(format!(
                "{} --> {}",
                srt_time(seg.start_ms),
                srt_time(seg.end_ms)
            ));
            lines.push(seg.text.trim().to_string());
            lines.push(String::new());
            idx += 1;
        }
        let content = lines.join("\n");
        validate_srt_content(&content, output)?;
        fs::write(output, content)
            .map_err(|e| anyhow::anyhow!("写入 SRT {} 失败: {}", output.display(), e))?;
        Ok(())
    }
}

/// 毫秒 → SRT 时间 `HH:MM:SS,mmm` (镜像 TS `srtTime`)。
fn srt_time(ms: u32) -> String {
    let total_secs = ms / 1000;
    let hh = total_secs / 3600;
    let mm = (total_secs % 3600) / 60;
    let ss = total_secs % 60;
    let mmm = ms % 1000;
    format!("{hh:02}:{mm:02}:{ss:02},{mmm:03}")
}

/// SRT 预检 (镜像 TS `validateSrtContent`): 在交给 ffmpeg 前拦截内容问题。
pub fn validate_srt_content(content: &str, file_path: &Path) -> anyhow::Result<()> {
    let raw_lines: Vec<&str> = content.split('\n').collect();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in raw_lines {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    if blocks.is_empty() {
        return Err(anyhow::anyhow!(
            "SRT 预检失败 ({}): 文件为空，没有任何字幕块",
            file_path.display()
        ));
    }

    for (i, block) in blocks.iter().enumerate() {
        let block_no = i + 1;
        if block.len() < 2 {
            return Err(anyhow::anyhow!(
                "SRT 预检失败 ({}): 第 {} 块结构非法，期望 \"序号 / 时间轴 / 文本\"，实际 {} 行",
                file_path.display(),
                block_no,
                block.len()
            ));
        }
        let parts: Vec<&str> = block[1].split(" --> ").collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "SRT 预检失败 ({}): 第 {} 块时间轴分隔符非法",
                file_path.display(),
                block_no
            ));
        }
        if !is_srt_time(parts[0]) || !is_srt_time(parts[1]) {
            return Err(anyhow::anyhow!(
                "SRT 预检失败 ({}): 第 {} 块时间轴格式非法: \"{}\" (应为 HH:MM:SS,mmm --> HH:MM:SS,mmm)",
                file_path.display(),
                block_no,
                block[1]
            ));
        }
        let s = parse_srt_time(parts[0]);
        let e = parse_srt_time(parts[1]);
        if !(e > s) {
            return Err(anyhow::anyhow!(
                "SRT 预检失败 ({}): 第 {} 块时间轴非法，结束时间必须晚于开始时间 ({})",
                file_path.display(),
                block_no,
                block[1]
            ));
        }
        if block[2].contains('\0') {
            return Err(anyhow::anyhow!(
                "SRT 预检失败 ({}): 第 {} 块文本含 NUL 字符",
                file_path.display(),
                block_no
            ));
        }
    }
    Ok(())
}

/// 校验 `HH:MM:SS,mmm` 格式 (无正则依赖)。
fn is_srt_time(t: &str) -> bool {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let hh = parts[0].parse::<u32>().ok();
    let mm = parts[1].parse::<u32>().ok();
    let (ss, ms) = {
        let rest: Vec<&str> = parts[2].split(',').collect();
        if rest.len() != 2 {
            return false;
        }
        (rest[0].parse::<u32>().ok(), rest[1].parse::<u32>().ok())
    };
    hh.is_some() && mm.is_some() && ss.is_some() && ms.is_some() && ms.unwrap() < 1000
}

fn parse_srt_time(t: &str) -> u32 {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let h: u32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(0);
    let rest: Vec<&str> = parts[2].split(',').collect();
    let s: u32 = rest.first().and_then(|x| x.parse().ok()).unwrap_or(0);
    let ms: u32 = rest.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    (h * 3600 + m * 60 + s) * 1000 + ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn subtitle_segments_write_srt() {
        // 写到 crate 下的 tmp/（gitignore: */tmp/*），不用系统 /tmp（AGENTS.md 规则）
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(format!("ld_ss_srt_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let p = std::path::Path::new(&dir).join("out.srt");
        let segs = SubtitleSegments(vec![
            SubtitleSegment {
                text: "你好".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            SubtitleSegment {
                text: "世界".into(),
                start_ms: 1000,
                end_ms: 2500,
            },
        ]);
        segs.write_srt(&p).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(content.contains("你好"));
        assert!(content.contains("世界"));
        assert!(content.contains("00:00:00,000 --> 00:00:01,000"));
        assert!(content.starts_with("1\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `write_srt` → `read_srt` 往返：`SubtitleSegment` 只有 `text` / `start_ms` /
    /// `end_ms`，三者都能在 SRT 文本里无损表达。
    #[test]
    fn read_srt_roundtrips_write_srt() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(format!("ld_srt_rt_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let p = std::path::Path::new(&dir).join("rt.srt");
        let original = SubtitleSegments(vec![
            SubtitleSegment {
                text: "你好".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            SubtitleSegment {
                text: "世界".into(),
                start_ms: 1000,
                end_ms: 2500,
            },
        ]);
        original.write_srt(&p).unwrap();
        let back = SubtitleSegments::read_srt(&p).unwrap();
        assert_eq!(back.0.len(), 2);
        assert_eq!(back.0[0].text, "你好");
        assert_eq!(back.0[0].start_ms, 0);
        assert_eq!(back.0[0].end_ms, 1000);
        assert_eq!(back.0[1].text, "世界");
        assert_eq!(back.0[1].start_ms, 1000);
        assert_eq!(back.0[1].end_ms, 2500);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_rejects_bad_time_order() {
        let dir = std::env::temp_dir()
            .join(format!("ld_srt_bad_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let p = std::path::Path::new(&dir).join("bad.srt");
        // 结束早于开始
        let content = "1\n00:00:02,000 --> 00:00:01,000\n文本\n".to_string();
        let f = std::fs::File::create(&p).unwrap();
        let mut w = std::io::BufWriter::new(f);
        w.write_all(content.as_bytes()).unwrap();
        drop(w);
        let err = validate_srt_content(&content, &p).unwrap_err();
        assert!(err.to_string().contains("结束时间必须晚于开始时间"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn srt_time_format() {
        assert_eq!(srt_time(0), "00:00:00,000");
        assert_eq!(srt_time(1000), "00:00:01,000");
        assert_eq!(srt_time(61_250), "00:01:01,250");
        assert_eq!(srt_time(3_661_009), "01:01:01,009");
    }
}
