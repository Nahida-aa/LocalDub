//! generate_meta 命令: 用 LLM 从 `mix_video/zh.srt` 生成 `mix_video/meta.md` (视频章节摘要)。
//!
//! 独立 task action, 必要参数 `taskDir`。数据源:
//! - 字幕: `{task_dir}/mix_video/zh.srt` (SRT, 含时间戳)
//! - 视频元数据: `{task_dir}/ytdlp_info.json` (yt-dlp --dump-json, 有 title/uploader/upload_date/webpage_url)
//!
//! LLM 输出直接写 markdown 到 `mix_video/meta.md` (参照现有 meta.md 格式)。

use std::path::Path;

use anyhow::Context;

use crate::stages::mix_video::read_srt_file_to_segs;
use crate::stages::utils::srt::SrtSeg;
use crate::stages::utils::ensure_dir;

/// 系统提示: 让 LLM 根据带时间戳的字幕输出结构化章节摘要 (JSON)。
///
/// Rust 侧负责组装 `## title` / `## description` 的固定格式, LLM 只输出
/// `title_translation` (标题中文翻译) 和 `chapters` (时间戳 + 简短标题 + 内容摘要)。
const META_SYSTEM_PROMPT: &str = r#"你是视频章节摘要助手。根据给定的带时间戳字幕, 输出 JSON 对象:

{
  "title_translation": "标题的中文翻译(不包含作者)",
  "chapters": [
    { "time": "MM:SS", "title": "该段简短中文标题", "summary": "该段内容摘要(2-4句)" }
  ]
}

要求:
1. title_translation 把视频标题翻译成中文, 不含作者名
2. chapters 按时间顺序组织, 覆盖整个视频主要内容, time 用 MM:SS 时间戳
3. 每个 chapter 的 title 是该时间段的简短概括, summary 是该段内容摘要
4. 只输出 JSON, 不要 markdown 代码块, 不要额外解释"#;

/// LLM 输出的结构化章节摘要 (解析自 JSON)。
#[derive(serde::Deserialize)]
struct MetaResult {
    title_translation: String,
    chapters: Vec<MetaChapter>,
}

#[derive(serde::Deserialize)]
struct MetaChapter {
    time: String,
    title: String,
    summary: String,
}

/// 从 ytdlp_info.json 提取的元数据。
struct YtDlpInfo {
    title: String,
    uploader: String,
    upload_date: Option<String>,
    webpage_url: Option<String>,
}

/// 生成 `{task_dir}/mix_video/meta.md`。
///
/// LLM 输出结构化 (title_translation + chapters), Rust 组装固定格式:
/// `## title` (中文翻译 - 作者) + `## description` (元数据 + 章节摘要)。
pub fn generate_meta(task_dir: &str) -> anyhow::Result<()> {
    let srt_path = format!("{task_dir}/mix_video/zh.srt");
    let segs = read_srt_file_to_segs(&srt_path)
        .with_context(|| format!("读取字幕失败 (generate_meta 需要 {srt_path})"))?;
    if segs.is_empty() {
        return Err(anyhow::anyhow!("{srt_path} 无字幕段"));
    }

    let info = read_ytdlp_info(task_dir);
    let prompt = build_meta_prompt(&segs, info.as_ref().map(|i| i.title.as_str()));

    let opts = llm::ChatOptions {
        model: Some(config_rs::env::openai_model()),
        api_base: Some(config_rs::env::openai_base_url()),
        system_prompt: META_SYSTEM_PROMPT.to_string(),
        api_key: config_rs::env::openai_api_key(),
        max_tokens: Some(4096),
        temperature: Some(0.3),
    };
    println!("[generate_meta] 调用 LLM 生成 meta.md ({} 段字幕)...", segs.len());
    let raw = llm::chat_completions(&prompt, &opts)?;

    let result: MetaResult = serde_json::from_str(&raw)
        .with_context(|| format!("解析 LLM 输出为 JSON 失败: {}", truncate(&raw, 200)))?;

    let md = assemble_meta_md(&result, info.as_ref());

    let meta_path = format!("{task_dir}/mix_video/meta.md");
    if let Some(parent) = Path::new(&meta_path).parent() {
        ensure_dir(parent).with_context(|| format!("创建目录失败: {parent:?}"))?;
    }
    std::fs::write(&meta_path, &md)
        .with_context(|| format!("写入 meta.md 失败: {meta_path}"))?;
    println!("已生成 {}", meta_path);
    Ok(())
}

/// 组装 meta.md 完整内容 (title + description + 章节摘要)。
fn assemble_meta_md(result: &MetaResult, info: Option<&YtDlpInfo>) -> String {
    let uploader = info.map(|i| i.uploader.as_str()).unwrap_or("");

    let mut md = String::new();
    // title: Rust 组装固定格式
    md.push_str("## title\n\n");
    md.push_str(&format!("{} - {}\n\n", result.title_translation.trim(), uploader));

    // description: 元数据 (Rust) + 章节摘要 (LLM)
    md.push_str("## description\n\n");
    if let Some(info) = info {
        if !info.title.is_empty() {
            md.push_str(&format!("原视频：{}\n", info.title));
        }
        if !info.uploader.is_empty() {
            md.push_str(&format!("原作者：{}\n", info.uploader));
        }
        if let Some(d) = &info.upload_date {
            md.push_str(&format!("发布日期：{}\n", format_upload_date(d)));
        }
        if let Some(u) = &info.webpage_url {
            md.push_str(&format!("视频链接：{}\n", u));
        }
        md.push('\n');
    }
    for c in &result.chapters {
        md.push_str(&format!("{} {}\n", c.time.trim(), c.title.trim()));
        if !c.summary.trim().is_empty() {
            md.push_str(&format!("{}\n", c.summary.trim()));
        }
        md.push('\n');
    }
    md
}

/// 截断字符串 (错误信息用)。
fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        chars[..max].iter().collect::<String>() + "..."
    }
}

/// 读 `{task_dir}/ytdlp_info.json`, 提取元数据。文件缺失/解析失败返回 None (本地视频无)。
fn read_ytdlp_info(task_dir: &str) -> Option<YtDlpInfo> {
    let path = format!("{task_dir}/ytdlp_info.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    let title = get("title").unwrap_or_default();
    let uploader = get("uploader").unwrap_or_default();
    if title.is_empty() && uploader.is_empty() {
        return None;
    }
    Some(YtDlpInfo {
        title,
        uploader,
        upload_date: get("upload_date"),
        webpage_url: get("webpage_url"),
    })
}

/// 构造 LLM 用户 prompt: 视频标题 (供翻译) + 按时间戳排的字幕。
fn build_meta_prompt(segs: &[SrtSeg], title: Option<&str>) -> String {
    let mut p = String::new();
    if let Some(t) = title {
        p.push_str(&format!("视频标题：{}\n\n", t));
    }
    p.push_str("字幕内容（时间戳 文本）：\n");
    for s in segs {
        let ts = format_ts(s.start_ms);
        let text = if !s.dst.trim().is_empty() {
            s.dst.trim()
        } else {
            s.text.trim()
        };
        if !text.is_empty() {
            p.push_str(&format!("{ts} {text}\n"));
        }
    }
    p
}

/// 毫秒 → `MM:SS` (如 81200 → 01:21)。
fn format_ts(ms: u64) -> String {
    let total = ms / 1000;
    let mm = total / 60;
    let ss = total % 60;
    format!("{mm:02}:{ss:02}")
}

/// yt-dlp upload_date (如 "20260819") → "2026-08-19"; 非法原样返回。
fn format_upload_date(d: &str) -> String {
    let b = d.as_bytes();
    if b.len() == 8 && b.iter().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ts_variants() {
        assert_eq!(format_ts(0), "00:00");
        assert_eq!(format_ts(60_000), "01:00");
        assert_eq!(format_ts(81_200), "01:21");
        assert_eq!(format_ts(773_000), "12:53");
    }

    #[test]
    fn format_upload_date_variants() {
        assert_eq!(format_upload_date("20260819"), "2026-08-19");
        assert_eq!(format_upload_date("unknown"), "unknown");
    }

    #[test]
    fn build_meta_prompt_includes_title_and_subtitles() {
        let segs = vec![SrtSeg {
            start_ms: 160,
            end_ms: 4900,
            dst: "你知道 TypeScript 是什么样的".to_string(),
            text: "你知道 TypeScript 是什么样的".to_string(),
            actual_start: None,
            actual_end: None,
        }];
        let p = build_meta_prompt(&segs, Some("I tried Compiled Typescript"));
        assert!(p.contains("视频标题：I tried Compiled Typescript"));
        assert!(p.contains("00:00 你知道 TypeScript 是什么样的"));
    }

    #[test]
    fn assemble_meta_md_uses_llm_translation_and_info() {
        let result = MetaResult {
            title_translation: "我尝试了编译型 TypeScript".to_string(),
            chapters: vec![MetaChapter {
                time: "00:00".to_string(),
                title: "TypeScript 的本质".to_string(),
                summary: "介绍 TypeScript 作为转译语言。".to_string(),
            }],
        };
        let info = YtDlpInfo {
            title: "I tried Compiled Typescript".to_string(),
            uploader: "The PrimeTime".to_string(),
            upload_date: Some("20260819".to_string()),
            webpage_url: Some("https://youtu.be/2g63UXaynaA".to_string()),
        };
        let md = assemble_meta_md(&result, Some(&info));
        assert!(md.contains("## title"));
        assert!(md.contains("我尝试了编译型 TypeScript - The PrimeTime"));
        assert!(md.contains("## description"));
        assert!(md.contains("原视频：I tried Compiled Typescript"));
        assert!(md.contains("原作者：The PrimeTime"));
        assert!(md.contains("发布日期：2026-08-19"));
        assert!(md.contains("00:00 TypeScript 的本质"));
        assert!(md.contains("介绍 TypeScript 作为转译语言。"));
    }

    #[test]
    fn assemble_meta_md_without_info() {
        let result = MetaResult {
            title_translation: "标题".to_string(),
            chapters: vec![],
        };
        let md = assemble_meta_md(&result, None);
        assert!(md.contains("## title"));
        assert!(md.contains("标题 - ")); // uploader 为空
        assert!(!md.contains("原视频："));
    }
}
