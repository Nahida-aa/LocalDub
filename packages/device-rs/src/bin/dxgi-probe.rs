//! DXGI 显存探测 CLI：输出 JSON。
//!
//! 用法: `cargo run -p device-rs --bin dxgi-probe`
//!
//! 输出结构：
//! ```json
//! {
//!   "adapters": [ { "index": 0, "description": "...", "local": { "budgetBytes": ... }, ... } ],
//!   "gb": [ { "description": "...", "localTotalGb": ..., "localUsedGb": ... } ]
//! }
//! ```
//! 仅支持 Windows；其他平台输出错误并退出码 2。

fn main() {
    #[cfg(not(windows))]
    {
        eprintln!("dxgi-probe is only supported on Windows");
        std::process::exit(2);
    }

    #[cfg(windows)]
    {
        use device_rs::gpu::dxgi::{DxgiAdapterProbe, DxgiProbeSummary, probe_dxgi};
        use serde::Serialize;

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Output {
            adapters: Vec<DxgiAdapterProbe>,
            gb: Vec<DxgiProbeSummary>,
        }

        let probes = probe_dxgi();
        let gb = probes.iter().map(DxgiProbeSummary::from).collect();
        let output = Output {
            adapters: probes,
            gb,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".into())
        );
    }
}
