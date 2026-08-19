//! cookie 命令 (镜像 TS `packages/core/cmd/cookie/handler.ts`)。
//!
//! `command: cookie` 时写入 YouTube Netscape cookie 到 `<repo>/data/cookies/youtube.txt`
//! (路径经 `config_rs::path::models` 对齐 TS `YOUTUBE_COOKIE_PATH`)。
//! 当前仅支持 `service=youtube` / `action=set` (扩展点, 与 TS 一致)。

use std::io::Read;

use anyhow::{Context, anyhow};
use config_rs::path::models::youtube_cookie_path;
use serde::{Deserialize, Serialize};

/// cookie 命令参数 (镜像 TS `CookieArgsSchema`: action=set + service=youtube + content 可选)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CookieArgs {
    /// 动作: set (当前仅支持)
    #[serde(default)]
    pub action: CookieAction,
    /// 服务: youtube (当前仅支持)
    #[serde(default)]
    pub service: CookieService,
    /// Netscape cookie 内容; 为空则从 stdin 读取
    #[serde(default)]
    pub content: Option<String>,
}

/// cookie 动作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum CookieAction {
    #[default]
    Set,
}

/// cookie 所属服务
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum CookieService {
    #[default]
    Youtube,
}

/// 写 cookie 到磁盘 (镜像 TS `setCookie`)。
pub fn set_cookie(content: &str) -> anyhow::Result<()> {
    if content.trim().is_empty() {
        return Err(anyhow!("[Cookie] No content provided"));
    }
    let path = youtube_cookie_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建 cookie 目录失败: {dir:?}"))?;
    }
    std::fs::write(&path, content).with_context(|| format!("写入 cookie 失败: {path:?}"))?;
    Ok(())
}

/// 命令入口 (镜像 TS `cmdCookie`): content 为空时从 stdin 读。
pub fn cmd_cookie(args: &CookieArgs) -> anyhow::Result<()> {
    let content = match &args.content {
        Some(c) if !c.trim().is_empty() => c.clone(),
        _ => {
            eprintln!("Paste your Netscape cookie (Ctrl+D to finish):");
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("读取 stdin 失败")?;
            buf
        }
    };
    set_cookie(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cookie_rejects_empty() {
        assert!(set_cookie("").is_err());
        assert!(set_cookie("   \n  ").is_err());
    }

    #[test]
    fn youtube_cookie_path_structure() {
        // 路径应与 TS `YOUTUBE_COOKIE_PATH` = <repo>/data/cookies/youtube.txt 对齐。
        let p = youtube_cookie_path();
        let s = p.to_string_lossy();
        assert!(s.ends_with("data/cookies/youtube.txt"), "got {s}");
    }

    #[test]
    fn set_cookie_writes_and_reads_back() {
        // 直接调用 set_cookie 写真实路径 (data/cookies/youtube.txt), 读回校验后清理。
        // 避免污染仓库: 仅当测试前置条件允许时写真实 cookie 文件。
        let path = youtube_cookie_path();
        if path.exists() {
            // 已有真实 cookie, 不覆盖用户数据, 跳过写测
            return;
        }
        let content = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tx\tval\n";
        set_cookie(content).expect("set_cookie 应成功");
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, content);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn deserialize_cookie_args() {
        let a: CookieArgs =
            serde_json::from_str(r#"{"content":"abc","service":"youtube","action":"set"}"#)
                .unwrap();
        assert_eq!(a.action, CookieAction::Set);
        assert_eq!(a.service, CookieService::Youtube);
        assert_eq!(a.content.as_deref(), Some("abc"));

        let empty: CookieArgs = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(empty.action, CookieAction::Set);
        assert_eq!(empty.service, CookieService::Youtube);
        assert!(empty.content.is_none());
    }
}
