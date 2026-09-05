use serde::{Deserialize, Serialize};

use clap::ValueEnum;
use config_rs::servers::ServerType;

/// 服务器操作 (镜像 packages/core/servers/input.ts 的 action 枚举)
/// (serde 与 clap 参数值统一为 lowercase)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, ValueEnum,
)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lowercase")]
pub enum ServerAction {
    Status,
    Start,
    Stop,
    Discovery,
}

impl Default for ServerAction {
    fn default() -> Self {
        Self::Status
    }
}

/// servers 命令参数 (镜像 packages/core/servers/input.ts 的 ServersArgsSchema)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct ServersArgs {
    /// 服务器操作
    pub action: ServerAction,
    /// 指定操作的服务器, 不传则操作所有
    pub name: Option<ServerType>,
    /// start 前台模式: 继承终端 stdio (日志实时可见), Ctrl+C 直接终止;
    /// 默认 false = detach 后台 + 日志落盘 logs/server.log
    #[serde(default)]
    pub foreground: bool,
}

impl Default for ServersArgs {
    fn default() -> Self {
        Self {
            action: ServerAction::default(),
            name: None,
            foreground: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shape() {
        assert_eq!(
            serde_json::to_string(&ServersArgs::default()).unwrap(),
            r#"{"action":"status","name":null}"#
        );
        let v: ServersArgs = serde_json::from_str(r#"{"name":"voxcpm_torch_gradio"}"#).unwrap();
        assert_eq!(v.action, ServerAction::Status);
        assert!(matches!(v.name, Some(ServerType::VoxcpmTorchGradio)));
    }
}
