param(
  [switch]$Dev  # development mode (skip DB init)
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "=== LocalDub 环境安装 (Windows) ==="
Write-Host "项目根: $RepoRoot"
Write-Host ""

# ── 检测前置 ──────────────────────────────────────
$Missing = @()
if (-not (Get-Command "bun" -ErrorAction SilentlyContinue))    { $Missing += "bun" }
if (-not (Get-Command "ffmpeg" -ErrorAction SilentlyContinue))  { $Missing += "ffmpeg" }
if (-not (Get-Command "python" -ErrorAction SilentlyContinue))  { $Missing += "python" }

if ($Missing.Count -gt 0) {
  Write-Host "[ERROR] 缺少以下命令，请先安装: $($Missing -join ', ')" -ForegroundColor Red
  exit 1
}
Write-Host "[OK] bun / ffmpeg / python 均已安装" -ForegroundColor Green

# ── GPU 检测 ──────────────────────────────────────
$GpuMode = "cpu"
$nvidia = Get-Command "nvidia-smi.exe" -ErrorAction SilentlyContinue
if ($nvidia) {
  try {
    $null = & $nvidia.Source 2>$null
    if ($LASTEXITCODE -eq 0) { $GpuMode = "cuda" }
  } catch {}
}
Write-Host "[GPU] 检测到: $GpuMode" -ForegroundColor Green
Write-Host ""

# ── .env ──────────────────────────────────────────
if (-not (Test-Path ".env")) {
  Copy-Item ".env.example" ".env"
  Write-Host "[INIT] 已创建 .env（请按需编辑）" -ForegroundColor Yellow
} else {
  Write-Host "[SKIP] .env 已存在"
}

# ── Python venv ──────────────────────────────────
if (-not (Test-Path ".venv")) {
  Write-Host "[PY] 创建虚拟环境..."
  python -m venv .venv
}
$activate = Join-Path $RepoRoot ".venv\Scripts\Activate.ps1"
. $activate
Write-Host "[PY] 虚拟环境: $(Get-Command python).Source"

Write-Host "[PY] 升级 pip..."
pip install --quiet --upgrade pip

$TorchIndex = ""
if ($GpuMode -eq "cpu") {
  $TorchIndex = "--index-url https://download.pytorch.org/whl/cpu"
}

Write-Host "[PY] 安装 Python 依赖 ($GpuMode)..."
pip install -r requirements.txt --quiet @($TorchIndex | Where-Object { $_ })

Write-Host "[PY] 完成" -ForegroundColor Green

# ── JS 依赖 ──────────────────────────────────────
if (-not (Test-Path "node_modules")) {
  Write-Host "[JS] bun install..."
  bun install
} else {
  Write-Host "[JS] node_modules 已存在"
}

# ── 读取 config.json ──────────────────────────────────
$ConfigPath = Join-Path $RepoRoot "packages\cli\config.json"
if (Test-Path $ConfigPath) {
    $Config = Get-Content $ConfigPath | ConvertFrom-Json
    $SepRuntime = $Config.stages.separate.runtime
    $TtsRuntime = $Config.stages.tts.runtime
    Write-Host "[CFG] separate.runtime=$SepRuntime, tts.runtime=$TtsRuntime" -ForegroundColor Cyan
} else {
    $SepRuntime = $null
    $TtsRuntime = $null
    Write-Host "[CFG] config.json not found, skipping submodule init" -ForegroundColor Yellow
}

# ── 子模块初始化 ──────────────────────────────────────
$SubmodulesToInit = @()
if ($SepRuntime -eq "pytorch" -or $SepRuntime -eq "ggml") {
    $SubmodulesToInit += "submodule/demucs"
}
if ($SepRuntime -eq "ggml") {
    $SubmodulesToInit += "submodule/demucs.cpp"
}
if ($TtsRuntime -eq "pytorch") {
    $SubmodulesToInit += "submodule/CosyVoice"
    $SubmodulesToInit += "submodule/VoxCPM"
}

# 去重
$SubmodulesToInit = $SubmodulesToInit | Sort-Object -Unique

foreach ($sm in $SubmodulesToInit) {
    $smPath = Join-Path $RepoRoot $sm.Replace("/", "\")
    if ((Test-Path (Join-Path $smPath ".git"))) {
        Write-Host "[SUB] $sm already initialized" -ForegroundColor Gray
    } else {
        Write-Host "[SUB] Initializing $sm ..." -ForegroundColor Yellow
        git submodule update --init $sm
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[SUB] Failed to init $sm" -ForegroundColor Red
        }
    }
}
if ($SubmodulesToInit.Count -eq 0) {
    Write-Host "[SUB] No submodules needed based on config" -ForegroundColor Gray
}

# ── GGML 构建检查 ─────────────────────────────────────
if ($SepRuntime -eq "ggml") {
    $GgmlBinary = Join-Path $RepoRoot "submodule\demucs.cpp\build\demucs_mt.cpp.main.exe"
    if (-not (Test-Path $GgmlBinary)) {
        Write-Host "[GGML] Binary not found, build required" -ForegroundColor Yellow
        # cmake 构建将在 runtime 时自动进行
    } else {
        Write-Host "[GGML] Binary exists: $GgmlBinary" -ForegroundColor Gray
    }
}

# ── 工作目录 ────────────────────────────────────────
$WorkfolderDir = if ($env:WORKFOLDER) { $env:WORKFOLDER } else { ".\workfolder" }
New-Item -ItemType Directory -Force -Path $WorkfolderDir | Out-Null
Write-Host "[OK] 工作目录: $WorkfolderDir" -ForegroundColor Green

Write-Host ""
Write-Host "=== 安装完成 ===" -ForegroundColor Green
Write-Host ""
Write-Host "使用方式:"
Write-Host "  编辑 .env 中的 API key 和配置"
Write-Host "  编辑 packages/cli/config.json 中的 video 链接"
Write-Host "  运行: cd packages/cli && bun run run-task.ts"
Write-Host ""
