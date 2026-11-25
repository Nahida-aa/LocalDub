# 运行 CLI。目前 Rust cli 仅初步实现了 command: task (读 input.jsonc →
# import/continue → 跑 pipeline), 尚未覆盖 check / servers / env。
# 若需要 check / servers / env, 暂回退到 TS 实现:
#     bun --cwd packages/cli run-task.ts
dev-cli:
    cargo run -p cli

dev-cli-ts:
    cd packages/cli && bun run tauri dev

dev-cli-workflow-start *url:
    cargo run -p cli -- workflow --action start {{if url != "" { "--url " + url } else { "" }}}

dev-cli-workflow-enqueue-dir *url:
    cargo run -p cli -- workflow --action enqueue_dir {{if url != "" { "--url " + url } else { "" }}}

dev-cli-discovery-server:
    cargo run -p cli -- servers --action discovery --name main

dev-cli-server-status:
    cargo run -p cli -- servers --action status --name main

dev-cli-server:
    cargo run -p cli -- servers --action start --name main --foreground

dev-cli-stop-server:
    cargo run -p cli -- servers --action stop --name main

dev-server:
    cargo run -p server

# 启动桌面端 (Tauri + Solid 前端)。等价于 packages/app 的 `dev:desktop`:
# 进入 packages/app 跑 `bun run tauri dev` (需先 bun install 装好依赖)。
# 用 cd 而非 `bun --cwd`, 后者对 `run <script>` 解析不可靠。
dev-desktop:
    cd packages/app && bun run tauri dev

dev-desktop-debug:
    cd packages/app && RUST_LOG=fs=debug bun run tauri dev

gen-input-schema:
    cargo run -p ld-core --bin gen-input-schema

gen-ts-sdk:
    cargo run -p sdk

# 运行环境检测 (镜像 TS env 命令)。
# `just run-env-check` = 按 input.jsonc 推断目标; `just run-env-check ffmpeg python` = 指定目标。
run-env-check *targets:
    cargo run -p cli -- env --action check {{if targets == "" { "" } else { "--targets " + targets }}}

run-env-ensure *targets:
    cargo run -p cli -- env --action ensure {{if targets == "" { "" } else { "--targets " + targets }}}

# 把某系列 workfolder/<series>/ 下各集成片拷贝到 out/<series>/<集>.mp4。
# 无参数时列出可导出的系列；`just export-series 某某` 导出单系列；
# `--all` 导出全部系列；`--force` 覆盖已存在的 out 文件。
# 也可用 WORKFOLDER 环境变量指定工作目录（默认 <repo>/workfolder）。
# 无参数时列出可导出系列: just export-series
export-series-0:
    bun scripts/export-series.ts

# 导出单系列, 或加 --all / --force (参数间空格分隔, 含空格的系列名用引号):
#   just export-series 剑
#   just export-series "错位未婚妻" --force
#   just export-series --all
export-series *args:
    bun scripts/export-series.ts {{args}}

# 生成剧集上传清单 xlsx (仅生成表格, 不上传; 数据源 out/<剧名>/<集数>.mp4)
# 列: 剧名 | 集数(总集数) | 姓名 | 时长(全剧求和) | 上传链接(网盘, 留空待填)
# 末尾一行总计; 依赖由脚本内 PEP 723 声明 (openpyxl), 不写入项目 pyproject.toml
#   just make-upload-sheet "师尊带我炸修真,梨落忘川雪" 安安
#   just make-upload-sheet "师尊带我炸修真" 安安 --out tmp/清单.xlsx
make-upload-sheet series name *args:
    uv run --no-project scripts/make-upload-sheet.py --series "{{series}}" --name "{{name}}" {{args}}

stats:
    scc . --exclude-dir node_modules,dist,build,target,venv,.venv,__pycache__,.git,vendor,out,cmake-build-debug,CMakeFiles --exclude-ext lock,json,md,yaml,yml,toml,ini,conf
