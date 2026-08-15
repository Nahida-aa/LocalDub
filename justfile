# 运行 CLI。目前 Rust cli 仅初步实现了 command: task (读 input.jsonc →
# import/continue → 跑 pipeline), 尚未覆盖 check / servers / env。
# 若需要 check / servers / env, 暂回退到 TS 实现:
#     bun --cwd packages/cli run-task.ts
run-cli:
    cargo run -p cli

run-cli-ts:
    cd packages/cli && bun run tauri dev

# 启动桌面端 (Tauri + Solid 前端)。等价于 packages/app 的 `dev:desktop`:
# 进入 packages/app 跑 `bun run tauri dev` (需先 bun install 装好依赖)。
# 用 cd 而非 `bun --cwd`, 后者对 `run <script>` 解析不可靠。
dev-desktop:
    cd packages/app && bun run tauri dev

gen-input-schema:
    cargo run -p ld-core --bin gen-input-schema
