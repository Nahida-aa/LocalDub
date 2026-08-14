demo:
    echo "This is a demo task."

demo-bun:
    bun -e "console.log('Hello from Bun!')"

demo-py:
    python -c "print('Hello from Python!')"

run_cli:
    bun --cwd packages/cli run-task.ts

gen-input-schema:
    cargo run -p ld-core --bin gen-input-schema
