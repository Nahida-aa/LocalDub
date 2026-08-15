# 运行 CLI。目前 Rust cli 仅初步实现了 command: task (读 input.jsonc →
# import/continue → 跑 pipeline), 尚未覆盖 check / servers / env。
# 若需要 check / servers / env, 暂回退到 TS 实现:
#     bun --cwd packages/cli run-task.ts
run-cli:
    cargo run -p cli

gen-input-schema:
    cargo run -p ld-core --bin gen-input-schema
