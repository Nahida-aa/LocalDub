demo:
    echo "This is a demo task."

demo-bun:
    bun -e "console.log('Hello from Bun!')"

demo-py:
    python -c "print('Hello from Python!')"

run_cli:
    bun --cwd packages/cli run-task.ts

gen_input_schema:
    cargo run -p core-rs --bin gen-input-schema
