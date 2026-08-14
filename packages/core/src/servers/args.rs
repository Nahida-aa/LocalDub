use serde::{Deserialize, Serialize};

use config_rs::servers::ServerType;

/// 服务器操作 (镜像 packages/core/servers/input.ts 的 action 枚举)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
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
}

impl Default for ServersArgs {
    fn default() -> Self {
        Self {
            action: ServerAction::default(),
            name: None,
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
        let v: ServersArgs = serde_json::from_str(r#"{"name":"demucs_torch_server"}"#).unwrap();
        assert_eq!(v.action, ServerAction::Status);
        assert!(matches!(v.name, Some(ServerType::DemucsTorchServer)));
    }
}
