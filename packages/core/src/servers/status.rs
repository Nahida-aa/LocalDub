//! 服务器运行状态 (模型级) 探测。
//!
//! 统一协议: 各服务器类型提供 `GET /status`, 返回 `ModelServerStatus` 形状
//! (镜像 vox-lab `voxcpm_torch_server/server.py` 的 `/status` 与 TS 侧
//! `packages/core/servers/type.ts` 的 `ModelServerStatus`)。主服务器 (packages/server)
//! 的原生 `/status` 由 axum_server 提供, voxcpm torch 由 vox-lab 提供。
//!
//! `probe_server_status` 只探测 mDNS **真实发现**的实例 (与 `cli servers status`
//!  一致, 不 fallback 默认端口, 避免「未运行却报默认端口」)。

use std::collections::HashMap;
use std::time::Duration;

use config_rs::servers::ServerType;
use serde::{Deserialize, Serialize};

use super::discovery::find_server_via_mdns_all;

/// 单个 HTTP 探测的超时。
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 服务器整体运行状态 (镜像 TS `ModelServerStatus.status`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ServerRunState {
    Running,
    Stopped,
    Timeout,
    Error,
}

/// 单个模型的加载状态 (镜像 TS `ModelStatus.status`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ModelLoadState {
    Ready,
    Loading,
    Error,
    Unloaded,
    Timeout,
}

/// 单个模型的状态 (镜像 TS `ModelStatus`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ModelStatus {
    pub status: ModelLoadState,
    pub device: String,
}

/// 服务器 + 模型级运行状态 (镜像 TS `ModelServerStatus`, 及 vox-lab `GET /status` body)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ModelServerStatus {
    /// 探测到的实例地址; 无 mDNS 实例时为 `None`。
    pub host: Option<String>,
    pub status: ServerRunState,
    /// 探测到的端口; 无 mDNS 实例时为 `None`。
    pub port: Option<u16>,
    pub uptime_s: u32,
    pub models: HashMap<String, ModelStatus>,
    pub message: Option<String>,
}

/// 探测某类型的服务器状态 (只探 mDNS 真实发现的实例)。
///
/// - 无 mDNS 实例 → `Stopped` + `message:"mDNS 未发现实例"`
/// - 按发现顺序逐实例探测, 首个 `Running` 即返回其完整状态
/// - 所有实例均未运行 → 返回首条的 `Stopped`/`Timeout`/`Error`
pub async fn probe_server_status(t: ServerType) -> ModelServerStatus {
    let list = find_server_via_mdns_all(t, None).await;
    if list.is_empty() {
        return ModelServerStatus {
            host: None,
            status: ServerRunState::Stopped,
            port: None,
            uptime_s: 0,
            models: HashMap::new(),
            message: Some("mDNS 未发现实例".to_string()),
        };
    }

    let mut last: Option<ModelServerStatus> = None;
    for (host, port) in list {
        let s = probe_one(&host, port).await;
        if s.status == ServerRunState::Running {
            return s;
        }
        last = Some(s);
    }
    last.unwrap_or_else(|| ModelServerStatus {
        host: None,
        status: ServerRunState::Stopped,
        port: None,
        uptime_s: 0,
        models: HashMap::new(),
        message: None,
    })
}

/// 探测单个实例的 `GET /status`, 解析完整状态。
async fn probe_one(host: &str, port: u16) -> ModelServerStatus {
    let url = format!("http://{host}:{port}/status");
    let Some(client) = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .ok()
    else {
        return ModelServerStatus {
            host: Some(host.to_string()),
            status: ServerRunState::Error,
            port: Some(port),
            uptime_s: 0,
            models: HashMap::new(),
            message: Some("构建 HTTP 客户端失败".to_string()),
        };
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            let status = if e.is_timeout() {
                ServerRunState::Timeout
            } else {
                ServerRunState::Stopped
            };
            return ModelServerStatus {
                host: Some(host.to_string()),
                status,
                port: Some(port),
                uptime_s: 0,
                models: HashMap::new(),
                message: Some(format!("{e}")),
            };
        }
    };

    if !resp.status().is_success() {
        return ModelServerStatus {
            host: Some(host.to_string()),
            status: ServerRunState::Error,
            port: Some(port),
            uptime_s: 0,
            models: HashMap::new(),
            message: Some(format!("HTTP {}", resp.status())),
        };
    }

    match resp.json::<ModelServerStatus>().await {
        Ok(mut s) => {
            s.host = Some(host.to_string());
            s.port = Some(port);
            s
        }
        Err(e) => ModelServerStatus {
            host: Some(host.to_string()),
            status: ServerRunState::Error,
            port: Some(port),
            uptime_s: 0,
            models: HashMap::new(),
            message: Some(format!("解析 /status 失败: {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 覆盖 vox-lab voxcpm_torch_server `/status` 的真实 body 形状。
    #[test]
    fn parse_voxcpm_status_body() {
        let body = r#"{"status":"running","port":19112,"uptime_s":42,"models":{"voxcpm":{"status":"ready","device":"cpu"}}}"#;
        let s: ModelServerStatus = serde_json::from_str(body).unwrap();
        assert_eq!(s.status, ServerRunState::Running);
        assert_eq!(s.port, Some(19112));
        assert_eq!(s.uptime_s, 42);
        assert_eq!(s.host, None);
        assert!(s.message.is_none());
        let m = s.models.get("voxcpm").unwrap();
        assert_eq!(m.status, ModelLoadState::Ready);
        assert_eq!(m.device, "cpu");
    }

    #[test]
    fn serde_roundtrip() {
        let s = ModelServerStatus {
            host: Some("127.0.0.1".into()),
            status: ServerRunState::Timeout,
            port: Some(19110),
            uptime_s: 7,
            models: HashMap::new(),
            message: Some("probe timeout".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: ModelServerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}