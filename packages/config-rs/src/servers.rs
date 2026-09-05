/// Server type identifiers for mDNS discovery.
///
/// 序列化为 snake_case (voxcpm_torch_gradio), 对齐 TS 侧
/// `packages/config/src/servers.ts` 的 serverTypeList。
/// (serde 与 clap 参数值统一为 snake_case)
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ServerType {
    /// LocalDub 主服务器 (packages/server, fnrpc 端点, 端口 19110)。
    Server,
    VoxcpmTorchGradio,
}

impl ServerType {
    /// All known server types.
    pub const ALL: &'static [ServerType] = &[ServerType::Server, ServerType::VoxcpmTorchGradio];

    /// Corresponding mDNS service name.
    pub fn service_name(self) -> &'static str {
        match self {
            ServerType::Server => "_ld-server._tcp.local",
            ServerType::VoxcpmTorchGradio => "_ld-voxcpm-py._tcp.local",
        }
    }

    /// Default TCP port for this server type.
    pub fn default_port(self) -> u16 {
        match self {
            ServerType::Server => 19110,
            ServerType::VoxcpmTorchGradio => 19112,
        }
    }
}
