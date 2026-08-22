# scripts/resource-monitor.ps1
# 实时资源监控：每 60 秒采样一次 CPU / 内存 / 磁盘 IO / GPU(独显)，追加写入 CSV 日志。
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File scripts/resource-monitor.ps1            # 守护模式(默认，前台循环)
#   powershell -ExecutionPolicy Bypass -File scripts/resource-monitor.ps1 -Once      # 单次采样后退出
#   powershell -ExecutionPolicy Bypass -File scripts/resource-monitor.ps1 -IntervalSeconds 60
#
# 日志: packages/tmp/resource-monitor/monitor-<YYYYMMDD>.csv
# 使用命名互斥体保证单实例；重复启动会自动退出(供自动化任务做守护用)。
# 建议用分离进程启动以驻留后台:
#   Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','<此脚本>' -WindowStyle Hidden

param(
  [switch]$Once,
  [int]$IntervalSeconds = 60
)

$ErrorActionPreference = 'SilentlyContinue'

# ---- 单实例保护 ----
$mutex = New-Object System.Threading.Mutex($false, 'LocalDub.ResourceMonitor')
if (-not $mutex.WaitOne(0)) {
  Write-Host "[resource-monitor] already running, exit."
  exit 0
}

# ---- 日志目录（gitignored 的临时目录） ----
$logDir = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'packages\tmp\resource-monitor'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$logFile = Join-Path $logDir ('monitor-{0:yyyyMMdd}.csv' -f (Get-Date))

if (-not (Test-Path $logFile)) {
  'timestamp,cpu_pct,mem_used_gb,mem_total_gb,disk_read_bps,disk_write_bps,gpu_vendor,gpu_name,gpu_util_pct,gpu_mem_used_mb,gpu_mem_total_mb,gpu_temp_c' |
    Out-File -FilePath $logFile -Encoding utf8
}

function Get-Sample {
  $ts = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')

  # CPU 使用率(%)
  $cpu = 0
  $c = Get-Counter '\Processor(_Total)\% Processor Time'
  if ($c) { $cpu = [math]::Round($c.CounterSamples[0].CookedValue, 1) }

  # 内存 (GB)
  $os = Get-CimInstance Win32_OperatingSystem
  $memTotalGb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 2)
  $memUsedGb = [math]::Round(($os.TotalVisibleMemorySize - $os.FreePhysicalMemory) / 1MB, 2)

  # 磁盘 IO (bytes/sec)
  $diskRead = 0; $diskWrite = 0
  $d = Get-Counter '\PhysicalDisk(_Total)\Disk Read Bytes/sec', '\PhysicalDisk(_Total)\Disk Write Bytes/sec'
  if ($d) {
    $diskRead = [int]$d.CounterSamples[0].CookedValue
    $diskWrite = [int]$d.CounterSamples[1].CookedValue
  }

  # GPU（独显）: 优先 nvidia-smi，其次 rocm-smi(Linux/WSL)
  $gpuVendor = ''; $gpuName = ''; $gpuUtil = ''; $gpuMemUsed = ''; $gpuMemTotal = ''; $gpuTemp = ''
  if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
    $raw = & nvidia-smi --query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu --format=csv,noheader,nounits
    if ($raw) {
      $p = $raw -split ','
      if ($p.Count -ge 5) {
        $gpuVendor = 'nvidia'
        $gpuName = $p[0].Trim()
        $gpuUtil = $p[1].Trim()
        $gpuMemUsed = $p[2].Trim()
        $gpuMemTotal = $p[3].Trim()
        $gpuTemp = $p[4].Trim()
      }
    }
  }
  if (-not $gpuVendor -and (Get-Command rocm-smi -ErrorAction SilentlyContinue)) {
    $line = (& rocm-smi --showuse --showmeminfo vram --showtemp) | Select-String -Pattern 'GPU\[' -SimpleMatch | Select-Object -First 1
    if ($line) {
      $gpuVendor = 'amd'
      $gpuName = (& rocm-smi --showproductname) | Select-String -Pattern 'Card Series:' | Select-Object -First 1
      $gpuName = if ($gpuName) { ($gpuName -replace '.*Card Series:\s*', '').Trim() } else { 'AMD GPU' }
      $tempM = $line -match '([\d.]+)°C'
      if ($tempM) { $gpuTemp = $Matches[1] }
      $pcts = [regex]::Matches($line, '(\d+)%') | ForEach-Object { $_.Groups[1].Value }
      if ($pcts.Count -ge 1) { $gpuUtil = $pcts[$pcts.Count - 1] }
      $memM = $line -match '(\d+)\s*MB used'
      if ($memM) { $gpuMemUsed = $Matches[1] }
    }
  }

  '{0},{1},{2},{3},{4},{5},{6},"{7}",{8},{9},{10},{11}' -f $ts, $cpu, $memUsedGb, $memTotalGb, $diskRead, $diskWrite, $gpuVendor, $gpuName, $gpuUtil, $gpuMemUsed, $gpuMemTotal, $gpuTemp
}

try {
  if ($Once) {
    Get-Sample | Out-File -FilePath $logFile -Append -Encoding utf8
    Write-Host "sampled -> $logFile"
    exit 0
  }

  Write-Host "[resource-monitor] started, sampling every ${IntervalSeconds}s -> $logFile"
  while ($true) {
    Get-Sample | Out-File -FilePath $logFile -Append -Encoding utf8
    Start-Sleep -Seconds $IntervalSeconds
  }
}
finally {
  $mutex.ReleaseMutex()
  $mutex.Dispose()
}
