use config_rs::root::repo_root;
use server::build_fn_rpc_router;

fn main() {
    let router = build_fn_rpc_router();

    let repo_root = repo_root();
    let output_path = repo_root.join("packages/sdk/fnrpc/bindings.ts");

    fnrpc::gen_ts_client::write_ts_client(&router, &output_path)
        .expect("failed to write fnrpc client");

    println!("Generated {}", output_path.display());
}
