//! SRT 写出 (镜像 TS `packages/core/utils/srt.ts` 的 `writeSrt` + `validateSrtContent`)。
//!
//! 仅实现「基础分支」: 每段输出一条 SRT (TS 的 `splitSubtitle` 多 fragment 切分逻辑未移植,
//! 因当前 Rust 流程的 segments 已是合适粒度, 与一致行为足够)。

use std::path::Path;

/// 单条 SRT 输入 (调用方负责从各 stage 结果映射到该结构)。
#[derive(Debug, Clone)]
pub struct SrtSeg {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 翻译后文本 (优先); use_source=true 时改用 source
    pub dst: String,
    /// 原文 (use_source 时使用)
    pub text: String,
    /// 实际开始/结束时间 (Timing 携带, 覆盖 start_ms/end_ms)
    pub actual_start: Option<u64>,
    pub actual_end: Option<u64>,
}

/// 毫秒 → SRT 时间 `HH:MM:SS,mmm` (镜像 TS `srtTime`)。
fn srt_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hh = total_secs / 3600;
    let mm = (total_secs % 3600) / 60;
    let ss = total_secs % 60;
    let mmm = ms % 1000;
    format!("{hh:02}:{mm:02}:{ss:02},{mmm:03}")
}

/// 写 SRT 文件 (镜像 TS `writeSrt`)。
///
/// - `use_source=true` 用 `text` (原文), 否则用 `dst` (翻译)
/// - 写盘前做 [`validate_srt_content`] 预检, 避免 ffmpeg 报含糊的 "Unable to open"
pub fn write_srt(segments: &[SrtSeg], output: &Path, use_source: bool) -> anyhow::Result<()> {
    let mut lines: Vec<String> = Vec::new();
    let mut idx: u32 = 1;
    for item in segments {
        let start = item.actual_start.unwrap_or(item.start_ms);
        let end = item.actual_end.unwrap_or(item.end_ms);
        let text = if use_source {
            item.text.trim().to_string()
        } else {
            let t = item.dst.trim();
            if t.is_empty() {
                item.text.trim().to_string()
            } else {
                t.to_string()
            }
        };
        if text.is_empty() {
            continue;
        }
        lines.push(idx.to_string());
        lines.push(format!("{} --> {}", srt_time(start), srt_time(end)));
        lines.push(text);
        lines.push(String::new());
        idx += 1;
    }
    let content = lines.join("\n");
    validate_srt_content(&content, output)?;
    std::fs::write(output, content)
        .map_err(|e| anyhow::anyhow!("写入 SRT {} 失败: {e}", output.display()))?;
    Ok(())
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

fn parse_srt_time(t: &str) -> u64 {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let h: u64 = parts[0].parse().unwrap_or(0);
    let m: u64 = parts[1].parse().unwrap_or(0);
    let rest: Vec<&str> = parts[2].split(',').collect();
    let s: u64 = rest.first().and_then(|x| x.parse().ok()).unwrap_or(0);
    let ms: u64 = rest.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    (h * 3600 + m * 60 + s) * 1000 + ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn seg(start: u64, end: u64, dst: &str, text: &str) -> SrtSeg {
        SrtSeg {
            start_ms: start,
            end_ms: end,
            dst: dst.to_string(),
            text: text.to_string(),
            actual_start: None,
            actual_end: None,
        }
    }

    #[test]
    fn write_srt_roundtrip() {
        let dir = std::env::temp_dir()
            .join(format!("ld_srt_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let p = std::path::Path::new(&dir).join("out.srt");
        let segs = vec![
            seg(0, 1000, "你好", "hello"),
            seg(1000, 2500, "世界", "world"),
        ];
        write_srt(&segs, &p, false).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(content.contains("你好"));
        assert!(content.contains("世界"));
        assert!(content.contains("00:00:00,000 --> 00:00:01,000"));
        assert!(content.contains("00:00:01,000 --> 00:00:02,500"));
        // 块号自增
        assert!(content.starts_with("1\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_srt_use_source() {
        let dir = std::env::temp_dir()
            .join(format!("ld_srt_src_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(&dir).unwrap();
        let p = std::path::Path::new(&dir).join("out.srt");
        // dst 空 → 回退 text
        let segs = vec![seg(0, 1000, "", "原文")];
        write_srt(&segs, &p, false).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(content.contains("原文"));
        // use_source=true → 用 text
        let segs2 = vec![seg(0, 1000, "译文", "原文")];
        write_srt(&segs2, &p, true).unwrap();
        let content2 = std::fs::read_to_string(&p).unwrap();
        assert!(content2.contains("原文"));
        assert!(!content2.contains("译文"));
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
