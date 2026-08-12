/// Server type identifiers for mDNS discovery.
///
/// 序列化为 snake_case (voxcpm_torch_gradio / demucs_torch_server), 对齐 TS 侧
/// `packages/config/src/servers.ts` 的 serverTypeList。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ServerType {
    VoxcpmTorchGradio,
    DemucsTorchServer,
}

impl ServerType {
    /// All known server types.
    pub const ALL: &'static [ServerType] =
        &[ServerType::VoxcpmTorchGradio, ServerType::DemucsTorchServer];

    /// Corresponding mDNS service name.
    pub fn service_name(self) -> &'static str {
        match self {
            ServerType::VoxcpmTorchGradio => "_ld-voxcpm-py._tcp.local",
            ServerType::DemucsTorchServer => "_ld-demucs-py._tcp.local",
        }
    }

    /// Default TCP port for this server type.
    pub fn default_port(self) -> u16 {
        match self {
            ServerType::VoxcpmTorchGradio => 19112,
            ServerType::DemucsTorchServer => 19109,
        }
    }
}
