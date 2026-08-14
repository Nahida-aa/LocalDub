# 迁移备忘: axum+fnrpc 从 app-tauri → packages/server

## 目标结构

```
packages/server/
  Cargo.toml            # fnrpc/fnrpc-axum/axum/tower-http/tokio/futures/serde/specta/tracing
                        # ld-core/config-rs/device-rs/fs; [lib]+默认bin+[[bin]] gen-fnrpc
  src/
    lib.rs              # pub mod 全部 + pub fn start() / build_axum_router()
    main.rs             # 独立 bin: tokio::main → AppState::new → build router → start(19110)
    ctx.rs              # AppState + Ctx (repo_root: CARGO_MANIFEST_DIR 2级 parent = repo root)
    commands.rs         # torch/voxcpm 进程管理, device_info, input 读写
    feat/mod.rs         # demo, file_op, other, servers, tasks (含 tasks/{mod,log,tree}.rs)
    fnrpc_func.rs       # 原 integrations/fnrpc_func.rs (build_fn_rpc_router)
    fnrpc_axum.rs       # 原 integrations/fnrpc_axum.rs (build_axum_router)
    axum_server.rs      # 原 server.rs (start), 避免与 crate 名 `server` 冲突→内部模块名定 axum_server
  src/bin/gen_fnrpc.rs  # 输出到 repo_root()/packages/app/src/integrations/fnrpc/bindings.ts
```

## 步骤

1. mkdir server/src/{feat/tasks}; 把 app-tauri 的 ctx/commands/feat/integrations/{fnrpc_func,fnrpc_axum}.rs/server.rs 平移
2. server/src/lib.rs 汇总导出; main.rs 独立入口
3. ctx.rs: repo_root = manifest.parent().parent() (2级)
4. app Cargo.toml: 删 server.rs/commands/feat/integrations 文件, 加 `server = { path = "../../server" }`,
   依赖瘦身 (移除 axum/tower-http/fnrpc-axum/ld-core/config-rs/device-rs/fs/futures/specta/tokio/tower)
5. app lib.rs: use server::*; FnrpcTauriState + generate_handler!(server::ctx::Ctx); HeaderMap::new() 保留 axum http 或 Default::default()
6. gen-fnrpc bin 移到 server; app package.json 改 `cargo run -p server --bin gen-fnrpc`
7. 验证: cargo check -p server / -p app; gen-fnrpc 后 git diff bindings.ts 应空

## 坑

- server.rs 文件名与 crate name `server` 冲突 → 内部模块名 axum_server, lib.rs 里 re-export
- Ctx/AppState 必须 pub (供 app 的 fnrpc_tauri 注册用)
- tracing-subscriber init 放哪个 bin 都行, lib 不含
- tree.rs 单测随迁仍可跑 (tokio::test)
- dev 下前端 vite(1420) fetch axum(19110) 需 CORS: layer 已在 fnrpc_axum 里 permissive
