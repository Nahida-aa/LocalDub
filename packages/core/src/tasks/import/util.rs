//! import 阶段共享助手 (镜像 TS `packages/core/tasks/import/utils.ts`)。
//!
//! 同步实现 (Rust CLI 用 blocking, 不引 tokio)。yt-dlp 直接 spawn 系统已装的
//! `yt-dlp` 二进制 (与 TS `spawnSync("yt-dlp", ...)` 一致); ffmpeg 转码/抽音频也
//! 直接 spawn `ffmpeg` 二进制, argv 与 TS 相同; 仅帧率探测用 `ffmpeg-next` (贴合
//! 仓库现有用法)。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};
use tracing::{info, warn};

use crate::context::VideoSource;

/// 判定结果: groupId / taskId / source / yt-dlp 扩展参数 / 标题。
#[derive(Debug, Clone)]
pub struct AutoInfo {
    pub group_id: String,
    pub task_id: String,
    pub source: VideoSource,
    pub yt_dlp_ext_args: Vec<String>,
    pub title: Option<String>,
}

pub fn is_youtube_url(url: &str) -> bool {
    extract_youtube_id(url).is_some()
}

pub fn is_bilibili_url(url: &str) -> bool {
    extract_bilibili_id(url).is_some()
}

/// 极简 URL 解析: 仅取 host / path / 指定 query 参数。避免引入 `url` crate。
struct SimpleUrl<'a> {
    host: String,
    path: &'a str,
    query: Vec<(&'a str, &'a str)>,
}

fn parse_url(url: &str) -> Option<SimpleUrl<'_>> {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let (authority, rest) = without_scheme
        .split_once('/')
        .unwrap_or((without_scheme, ""));
    let (host, _userinfo_port) = authority.rsplit_once('@').unwrap_or((authority, ""));
    let host = host.split(':').next().unwrap_or(host).to_lowercase();
    let path = if rest.is_empty() { "/" } else { rest };
    let query = match path.split_once('?') {
        Some((p, q)) => {
            let params = q
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .collect::<Vec<_>>();
            (p, params)
        }
        None => (path, vec![]),
    };
    Some(SimpleUrl {
        host,
        path: query.0,
        query: query.1,
    })
}

fn extract_youtube_id(url: &str) -> Option<String> {
    let u = parse_url(url)?;
    let path = u.path.trim_start_matches('/');
    if u.host == "youtu.be" || u.host == "www.youtu.be" {
        let candidate = path.split('/').next().unwrap_or("");
        if is_youtube_id(candidate) {
            return Some(candidate.to_string());
        }
    }
    if !u.host.contains("youtube.com") {
        return None;
    }
    if let Some((_, v)) = u.query.iter().find(|(k, _)| *k == "v") {
        if is_youtube_id(v) {
            return Some(v.to_string());
        }
    }
    let parts: Vec<&str> = path.split('/').collect();
    for prefix in ["shorts", "embed", "live"] {
        if parts.len() >= 2 && parts[0] == prefix && is_youtube_id(parts[1]) {
            return Some(parts[1].to_string());
        }
    }
    None
}

fn extract_bilibili_id(url: &str) -> Option<String> {
    let u = parse_url(url)?;
    let hosts: HashSet<&str> = ["bilibili.com", "www.bilibili.com", "m.bilibili.com"]
        .into_iter()
        .collect();
    if !hosts.contains(u.host.as_str()) {
        return None;
    }
    let bytes = u.path.as_bytes();
    // 匹配 BV 后跟 10 个 [A-Za-z0-9]
    let mut i = 0;
    while i + 11 < bytes.len() + 1 {
        let s = &u.path[i..];
        if let Some(rest) = s.strip_prefix("BV") {
            let id = &rest[..rest.len().min(10)];
            if id.len() == 10 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Some(format!("BV{id}"));
            }
        }
        i += 1;
        if i >= u.path.len() {
            break;
        }
    }
    None
}

fn is_youtube_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn extract_video_id(url: &str) -> String {
    if let Some(id) = extract_youtube_id(url).or_else(|| extract_bilibili_id(url)) {
        return id;
    }
    if let Some(name) = Path::new(url).file_name().and_then(|s| s.to_str()) {
        if let Some(stem) = Path::new(name).file_stem().and_then(|s| s.to_str()) {
            return stem.to_string();
        }
    }
    // 兜底: url 的确定性短串
    format!("{:x}", fnv1a(url.as_bytes()))
}

pub fn classify_source(url: &str) -> anyhow::Result<VideoSource> {
    if is_youtube_url(url) {
        return Ok(VideoSource::Youtube);
    }
    if is_bilibili_url(url) {
        return Ok(VideoSource::Bilibili);
    }
    if Path::new(url).exists() {
        return Ok(VideoSource::Local);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(VideoSource::Remote);
    }
    Err(anyhow!(
        "无法判定视频来源: 需为 YouTube/Bilibili 链接、已存在的本地文件路径或远程 URL"
    ))
}

fn parse_dir_and_id(path: &str) -> (String, String) {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return ("root".to_string(), "video".to_string());
    }
    let file_name = parts[parts.len() - 1];
    let id = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_string();
    let generic_dirs: HashSet<&str> = [
        "videos",
        "video",
        "downloads",
        "download",
        "media",
        "tmp",
        "temp",
        "uploads",
        "upload",
        "files",
        "workfolder",
        "local",
    ]
    .into_iter()
    .collect();
    let n = parts.len();
    // 直接父目录
    let parent = parts[n - 2];
    if generic_dirs.contains(parent) && n >= 3 {
        // 跳过通用父目录, 用祖父目录作为 group
        let grand = parts[n - 3];
        if !generic_dirs.contains(grand) {
            return (grand.to_string(), id);
        }
    }
    (parent.to_string(), id)
}

/// 解析 groupId / taskId / yt-dlp 扩展参数 (镜像 TS `autoGroupIdAndVideoId`)。
pub fn auto_group_id_and_video_id(url: &str) -> anyhow::Result<AutoInfo> {
    let source = classify_source(url)?;
    let mut ret = AutoInfo {
        group_id: "root".to_string(),
        task_id: "unset".to_string(),
        source,
        yt_dlp_ext_args: vec![],
        title: None,
    };

    match source {
        VideoSource::Local | VideoSource::Remote => {
            let (dir, id) = parse_dir_and_id(url);
            ret.group_id = dir;
            ret.task_id = id;
        }
        VideoSource::Youtube | VideoSource::Bilibili => {
            let is_yt = matches!(source, VideoSource::Youtube);
            if is_yt {
                if let Some(cookie) = youtube_cookie_path() {
                    if cookie.exists() {
                        ret.yt_dlp_ext_args.push("--cookies".into());
                        ret.yt_dlp_ext_args.push(cookie.to_string_lossy().into());
                    }
                }
                if let Ok(port) = std::env::var("YTDLP_PROXY_PORT") {
                    if !port.is_empty() {
                        ret.yt_dlp_ext_args.push("--proxy".into());
                        ret.yt_dlp_ext_args.push(format!("http://127.0.0.1:{port}"));
                    }
                }
            }
            if let Some(u) = parse_url(url) {
                if u.query.iter().any(|(k, _)| *k == "list") {
                    let index = u
                        .query
                        .iter()
                        .find(|(k, _)| *k == "list")
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_else(|| "1".to_string());
                    ret.yt_dlp_ext_args.push("--playlist-items".into());
                    ret.yt_dlp_ext_args.push(index);
                } else {
                    ret.yt_dlp_ext_args.push("--no-playlist".into());
                }
            } else {
                ret.yt_dlp_ext_args.push("--no-playlist".into());
            }

            // --dump-json 探测信息
            let mut info_args = vec!["--dump-json".to_string()];
            info_args.extend(ret.yt_dlp_ext_args.clone());
            info_args.push(url.to_string());

            let out = run_yt_dlp(&info_args)
                .with_context(|| "yt-dlp --dump-json 失败 (cookie 可能过期或 URL 无效)")?;
            let info: serde_json::Value = match serde_json::from_str(&out) {
                Ok(v) => v,
                Err(e) => {
                    warn!("[autoGroupIdAndVideoId] yt-dlp 输出非 JSON: {e}");
                    serde_json::Value::Null
                }
            };
            if info.is_null() {
                ret.task_id = extract_video_id(url);
            } else {
                let uploader = info.get("uploader").and_then(|v| v.as_str());
                let playlist_title = info.get("playlist_title").and_then(|v| v.as_str());
                let group_name = match (playlist_title, uploader) {
                    (Some(pt), Some(up)) => sanitize_text(&format!("{up}-{pt}")),
                    (Some(pt), None) => sanitize_text(pt),
                    (None, Some(up)) => sanitize_text(up),
                    (None, None) => "unknown".to_string(),
                };
                let video_id = info
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| extract_video_id(url));
                ret.group_id = group_name;
                ret.task_id = video_id;
                ret.title = info
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let task_dir = workfolder().join(&ret.group_id).join(&ret.task_id);
                if let Err(e) = std::fs::create_dir_all(&task_dir) {
                    warn!("[autoGroupIdAndVideoId] 创建 taskDir 失败: {e}");
                } else if let Err(e) = std::fs::write(
                    task_dir.join("ytdlp_info.json"),
                    serde_json::to_string_pretty(&info).unwrap_or_default(),
                ) {
                    warn!("[autoGroupIdAndVideoId] 写 ytdlp_info.json 失败: {e}");
                }
            }
        }
        VideoSource::Unknown => {}
    }
    Ok(ret)
}

pub fn copy_file_to_path(src: &str, target: &str) -> anyhow::Result<()> {
    std::fs::copy(src, target).with_context(|| format!("复制文件失败: {src} -> {target}"))?;
    Ok(())
}

/// 远程 HTTP 下载 (最小实现: 仅支持 http://, 复用系统 TCP)。
///
/// 注: 不含 TLS; https 远程 URL 请用 yt-dlp 或本地文件。与 TS `downloadRemoteVideo`
/// (fetch) 对齐, 覆盖 pipeline 常见的 http 直链场景。
pub fn download_remote_video(url: &str, task_dir: &str) -> anyhow::Result<String> {
    let u = parse_url(url).ok_or_else(|| anyhow!("非法 URL: {url}"))?;
    if u.host.is_empty() || (!url.starts_with("http://")) {
        return Err(anyhow!(
            "远程下载仅支持 http:// (https 请用 yt-dlp 或本地文件): {url}"
        ));
    }
    let port = url_port(url).unwrap_or(80);
    let path = if u.path.is_empty() { "/" } else { u.path };
    let filename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("video.mp4");
    let raw_path = Path::new(task_dir).join(filename);

    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect((u.host.as_str(), port))
        .with_context(|| format!("连接 {}:{port} 失败", u.host))?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        host = u.host
    );
    stream
        .write_all(req.as_bytes())
        .with_context(|| "发送 HTTP 请求失败")?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .with_context(|| "读取响应失败")?;
    let body = strip_http_body(&buf)?;
    std::fs::write(&raw_path, body).with_context(|| "写远程文件失败")?;
    Ok(raw_path.to_string_lossy().into())
}

fn url_port(url: &str) -> Option<u16> {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let authority = without_scheme
        .split_once('/')
        .map(|(a, _)| a)
        .unwrap_or(without_scheme);
    let host_only = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    host_only
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
}

fn strip_http_body(buf: &[u8]) -> anyhow::Result<Vec<u8>> {
    let s = std::str::from_utf8(buf).map_err(|_| anyhow!("响应非文本, 无法解析 HTTP 头"))?;
    let idx = s
        .find("\r\n\r\n")
        .ok_or_else(|| anyhow!("非法 HTTP 响应"))?;
    let header = &s[..idx];
    if !header.starts_with("HTTP/1.1 200") && !header.starts_with("HTTP/1.0 200") {
        return Err(anyhow!(
            "HTTP 响应非 200: {}",
            &header[..header.len().min(64)]
        ));
    }
    let offset = idx + 4;
    Ok(buf[offset..].to_vec())
}

/// 转码为 H.264 + AAC 的 mp4 (argv 与 TS `encodeToMp4` 一致)。
pub fn encode_to_mp4(input: &str, output: &str) -> anyhow::Result<()> {
    run_ffmpeg(&[
        "-i",
        input,
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-c:v",
        "libx264",
        "-preset",
        "fast",
        "-crf",
        "23",
        "-c:a",
        "aac",
        "-movflags",
        "+faststart",
        output,
    ])
}

/// 从视频抽取 wav (16bit/44.1k/双声道), 供下游 ASR/TTS (argv 与 TS 一致)。
pub fn extract_audio(video: &str, audio: &str) -> anyhow::Result<()> {
    run_ffmpeg(&[
        "-i",
        video,
        "-acodec",
        "pcm_s16le",
        "-ar",
        "44100",
        "-ac",
        "2",
        audio,
    ])
}

/// 用 ffmpeg-next 探测视频帧率 (对齐 subtitle-ocr-cli 的 probe 用法)。
pub fn probe_frame_rate(video: &str) -> time::FrameRate {
    if ffmpeg_next::init().is_err() {
        return time::FrameRate::FPS_30;
    }
    let ictx = match ffmpeg_next::format::input(Path::new(video)) {
        Ok(v) => v,
        Err(_) => return time::FrameRate::FPS_30,
    };
    match ictx.streams().best(ffmpeg_next::media::Type::Video) {
        Some(s) => {
            let r = s.avg_frame_rate();
            if r.numerator() != 0 && r.denominator() != 0 {
                time::FrameRate::new(r.numerator() as u32, r.denominator() as u32)
            } else {
                time::FrameRate::FPS_30
            }
        }
        None => time::FrameRate::FPS_30,
    }
}

/// spawn ffmpeg 二进制, 非零退出即报错 (含 stderr)。
pub fn run_ffmpeg(args: &[&str]) -> anyhow::Result<()> {
    // 统一加 `-y`: ffmpeg 默认不覆盖已存在输出文件 (非交互下会失败),
    // 转码/抽音频都是写新产物, 覆盖旧的合理 (避免残留坏文件).
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("-y");
    argv.extend_from_slice(args);
    run_binary("ffmpeg", &argv).map(|_| ())
}

/// spawn yt-dlp 二进制, 返回 stdout (用于 --dump-json); 非零退出即报错 (含 stderr)。
pub fn run_yt_dlp(args: &[String]) -> anyhow::Result<String> {
    let bin = std::env::var("YTDLP_BIN").unwrap_or_else(|_| "yt-dlp".to_string());
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_binary(&bin, &argv)
}

fn run_binary(bin: &str, args: &[&str]) -> anyhow::Result<String> {
    let t0 = std::time::Instant::now();
    let out = Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("无法执行 {bin} (PATH 中未找到? 需安装)"))?;
    let elapsed = t0.elapsed().as_secs_f32();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("{bin} 退出码 {:?}: {}", out.status.code(), stderr));
    }
    info!("[{bin}] 完成 ({elapsed:.1}s)");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn sanitize_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

fn youtube_cookie_path() -> Option<PathBuf> {
    // 与 TS YOUTUBE_COOKIE_PATH 对齐: <repo>/data/cookies/youtube.txt
    Some(config_rs::path::models::youtube_cookie_path())
}

fn workfolder() -> PathBuf {
    config_rs::path::paths::workfolder()
}

/// 确定性短哈希 (兜底 taskId), 避免引入额外依赖。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_source_various() {
        assert!(matches!(
            classify_source("https://www.youtube.com/watch?v=abcdefghij_").unwrap(),
            VideoSource::Youtube
        ));
        assert!(matches!(
            classify_source("https://www.bilibili.com/video/BV1xx411c7mD").unwrap(),
            VideoSource::Bilibili
        ));
        assert!(matches!(
            classify_source("https://example.com/video.mp4").unwrap(),
            VideoSource::Remote
        ));
        // 本地文件需真实存在; 临时文件验证 Local 分支
        let tmp = std::env::temp_dir().join("localdub_test_video.mp4");
        std::fs::write(&tmp, b"x").unwrap();
        assert!(matches!(
            classify_source(tmp.to_str().unwrap()).unwrap(),
            VideoSource::Local
        ));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn extract_youtube_id_variants() {
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/watch?v=abcdefghij_").as_deref(),
            Some("abcdefghij_")
        );
        assert_eq!(
            extract_youtube_id("https://youtu.be/abcdefghij_").as_deref(),
            Some("abcdefghij_")
        );
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/shorts/abcdefghij_").as_deref(),
            Some("abcdefghij_")
        );
        assert!(extract_youtube_id("https://example.com/x").is_none());
    }

    #[test]
    fn parse_dir_and_id_basic() {
        // /a/b/video.mp4 → dir 取直接父目录 b (与 TS parseDirAndId 一致)
        assert_eq!(
            parse_dir_and_id("/a/b/video.mp4"),
            ("b".to_string(), "video".to_string())
        );
        // workfolder/local 都是通用目录, 无更具体的祖父目录 → 退回直接父目录 local
        assert_eq!(
            parse_dir_and_id("/workfolder/local/video.mp4"),
            ("local".to_string(), "video".to_string())
        );
    }

    #[test]
    fn parse_dir_and_id_skips_generic_parent() {
        // /group/videos/clip.mp4 → group (videos 是通用目录, 跳过)
        assert_eq!(
            parse_dir_and_id("/group/videos/clip.mp4"),
            ("group".to_string(), "clip".to_string())
        );
    }

    #[test]
    fn extract_video_id_local_path() {
        assert_eq!(extract_video_id("/x/y/myclip.mp4"), "myclip");
    }
}
