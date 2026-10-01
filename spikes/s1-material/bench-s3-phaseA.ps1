# S3 phase A: bare-window performance baseline.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# WHAT IS MEASURED, AND HOW "COLD START" IS DEFINED
#
# The spike writes a READY json from Tauri's on_page_load callback. The harness
# records the process START time and the READY file's mtime, both from the same
# wall clock, and differences them:
#
#     cold_start_ms = ready_file_mtime - process_start_time
#
# Using file times rather than "poll until the file appears" removes the polling
# interval from the number. Polling would add up to one poll interval of noise,
# and on a 700ms budget that is not acceptable.
#
# FIRST vs SUBSEQUENT starts are reported separately: the first start pays for a
# cold WebView2, later ones reuse it. Averaging them into one figure would hide
# the thing the budget is actually about.
#
# MEMORY is sampled in TWO states, because the budget has two rows:
#   visible         -> window shown, settled
#   hidden after 5s -> ShowWindow(SW_HIDE), then 5 seconds
# The WebView2 process tree is matched by walking ParentProcessId up to our PID,
# so it does not depend on process names being unique.
#
# Usage:
#   powershell -File spikes/s1-material/bench-s3-phaseA.ps1
#   powershell -File spikes/s1-material/bench-s3-phaseA.ps1 -Runs 30
# ---------------------------------------------------------------------------

param(
    [int]$Runs = 30,
    [int]$SettleVisibleMs = 3000,
    [int]$HiddenWaitMs = 5000,
    [string]$OutDir = ""
)

$ErrorActionPreference = 'Stop'

$src = @"
using System;
using System.Runtime.InteropServices;
public class S3Win {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
"@
Add-Type -TypeDefinition $src

$spikeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeDir 'out' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$exe = Join-Path $spikeDir 'target\release\s1-material.exe'
if (-not (Test-Path $exe)) { throw "release exe not found; run 'cargo build --release' in $spikeDir first" }
$exeSize = (Get-Item $exe).Length
Write-Host ("exe : {0}  ({1:N2} MB)" -f $exe, ($exeSize / 1MB)) -ForegroundColor Cyan

# Snapshots the WebView2 process tree as a "process -> parent" table once per run.
function Get-ProcessTree {
    param([int]$RootPid)
    $all = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Select-Object ProcessId, ParentProcessId, Name
    $byParent = @{}
    foreach ($p in $all) {
        $pp = [int]$p.ParentProcessId
        if (-not $byParent.ContainsKey($pp)) { $byParent[$pp] = @() }
        $byParent[$pp] += $p
    }
    $result = @()
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($RootPid)
    $seen = @{}
    while ($queue.Count -gt 0) {
        $cur = [int]$queue.Dequeue()
        if ($seen.ContainsKey($cur)) { continue }
        $seen[$cur] = $true
        if ($byParent.ContainsKey($cur)) {
            foreach ($child in $byParent[$cur]) {
                $result += $child
                $queue.Enqueue([int]$child.ProcessId)
            }
        }
    }
    return $result
}

function Measure-TreeMemory {
    param([int]$RootPid)
    $desc = Get-ProcessTree -RootPid $RootPid
    $rootProc = Get-Process -Id $RootPid -ErrorAction SilentlyContinue
    $rootWs = 0
    if ($rootProc) { $rootWs = $rootProc.WorkingSet64 }
    $webWs = 0
    $webCount = 0
    foreach ($d in $desc) {
        if ($d.Name -like 'msedgewebview2*') {
            $wp = Get-Process -Id ([int]$d.ProcessId) -ErrorAction SilentlyContinue
            if ($wp) { $webWs += $wp.WorkingSet64; $webCount++ }
        }
    }
    return [pscustomobject]@{
        root_mb  = [Math]::Round($rootWs / 1MB, 1)
        web_mb   = [Math]::Round($webWs / 1MB, 1)
        total_mb = [Math]::Round(($rootWs + $webWs) / 1MB, 1)
        web_procs = $webCount
    }
}

function Invoke-Run {
    param([int]$Index, [bool]$IsFirst)

    $ready = Join-Path $OutDir ("ready-{0:D2}.json" -f $Index)
    $result = Join-Path $OutDir ("bench-result-{0:D2}.json" -f $Index)
    foreach ($f in @($ready, $result)) { if (Test-Path $f) { Remove-Item $f -Force } }

    # -- bare window: minimal page, no material, decorations off (our shipping shape)
    $argList = @('--material', 'none', '--no-page', '--result', $result, '--ready-json', $ready)
    $quoted = @()
    foreach ($a in $argList) { if ($a -match '\s') { $quoted += '"' + $a + '"' } else { $quoted += $a } }

    $t0 = Get-Date
    $proc = Start-Process -FilePath $exe -ArgumentList $quoted -PassThru
    $rootPid = $proc.Id

    try {
        # wait for the ready signal
        $deadline = $t0.AddSeconds(30)
        while (-not (Test-Path $ready)) {
            if ($proc.HasExited) { throw "process exited (code $($proc.ExitCode)) before ready" }
            if ((Get-Date) -gt $deadline) { throw "timed out waiting for ready json" }
            Start-Sleep -Milliseconds 5
        }
        $tReady = Get-Date
        $readyMtime = (Get-Item $ready).LastWriteTime

        # both timestamps come from the same clock; take the earlier of the two
        # harness observations to stay conservative
        $byMtime = ($readyMtime - $t0).TotalMilliseconds
        $byPoll  = ($tReady - $t0).TotalMilliseconds
        $coldMs = [Math]::Min($byMtime, $byPoll)
        if ($coldMs -lt 0) { $coldMs = $byPoll }

        # visible, settled
        Start-Sleep -Milliseconds $SettleVisibleMs
        $vis = Measure-TreeMemory -RootPid $rootPid

        # hidden, then 5s
        $hwnd = $null
        if (Test-Path $ready) {
            $rj = Get-Content $ready -Raw -Encoding UTF8 | ConvertFrom-Json
            if ($rj.hwnd -and $rj.hwnd -ne 'unavailable') { $hwnd = [IntPtr][int64]$rj.hwnd }
        }
        $hid = $null
        if ($hwnd -and $hwnd -ne [IntPtr]::Zero) {
            [S3Win]::ShowWindow($hwnd, 0) | Out-Null   # SW_HIDE
            Start-Sleep -Milliseconds $HiddenWaitMs
            $hid = Measure-TreeMemory -RootPid $rootPid
        } else {
            Write-Host "  WARN: no hwnd, cannot measure hidden state" -ForegroundColor Yellow
        }

        $row = [pscustomobject]@{
            index          = $Index
            first          = $IsFirst
            cold_ms        = [Math]::Round($coldMs, 1)
            cold_by_mtime  = [Math]::Round($byMtime, 1)
            cold_by_poll   = [Math]::Round($byPoll, 1)
            vis_root_mb    = $vis.root_mb
            vis_web_mb     = $vis.web_mb
            vis_total_mb   = $vis.total_mb
            vis_web_procs  = $vis.web_procs
            hid_root_mb    = if ($hid) { $hid.root_mb } else { $null }
            hid_web_mb     = if ($hid) { $hid.web_mb } else { $null }
            hid_total_mb   = if ($hid) { $hid.total_mb } else { $null }
            hid_web_procs  = if ($hid) { $hid.web_procs } else { $null }
        }
        Write-Host ("  #{0:D2} cold={1,7:N1} ms   vis={2,6:N1} MB (web {3,6:N1} x{4})   hid={5}" -f `
            $Index, $row.cold_ms, $row.vis_total_mb, $row.vis_web_mb, $row.vis_web_procs, `
            $(if ($hid) { "{0,6:N1} MB" -f $hid.total_mb } else { "n/a" }))
        return $row
    }
    finally {
        if ($proc -and -not $proc.HasExited) { $proc.Kill(); $proc.WaitForExit(3000) | Out-Null }
        # give the OS a moment to reap the WebView2 children before the next run,
        # otherwise the next run's tree walk can pick up stale processes
        Start-Sleep -Milliseconds 700
    }
}

$rows = @()
for ($i = 1; $i -le $Runs; $i++) {
    Write-Host ("-- run {0}/{1} --" -f $i, $Runs) -ForegroundColor Cyan
    $rows += Invoke-Run -Index $i -IsFirst ($i -eq 1)
}

function Stat {
    param($values, [string]$name)
    $v = @($values | Where-Object { $_ -ne $null } | Sort-Object)
    if ($v.Count -eq 0) { return "$name : n/a" }
    $median = if ($v.Count % 2 -eq 1) { $v[[int]($v.Count / 2)] } else { ($v[$v.Count / 2 - 1] + $v[$v.Count / 2]) / 2 }
    $p95idx = [Math]::Min($v.Count - 1, [int][Math]::Ceiling($v.Count * 0.95) - 1)
    return ("{0,-10} median={1,8:N1}  p95={2,8:N1}  min={3,8:N1}  max={4,8:N1}" -f `
        $name, $median, $v[$p95idx], $v[0], $v[$v.Count - 1])
}

$all = $rows
$subsequent = @($rows | Where-Object { -not $_.first })

Write-Host ""
Write-Host "==== S3 phase A: bare window ====" -ForegroundColor Cyan
Write-Host ("  runs={0}  (first-start excluded from the medians below)" -f $Runs)
Write-Host ("  " + (Stat -values @($rows | ForEach-Object { $_.cold_ms }) -name 'cold(first)'))
Write-Host ("  " + (Stat -values @($subsequent | ForEach-Object { $_.cold_ms }) -name 'cold(warm)'))
Write-Host ("  " + (Stat -values @($subsequent | ForEach-Object { $_.vis_total_mb }) -name 'mem(vis)'))
Write-Host ("  " + (Stat -values @($subsequent | ForEach-Object { $_.vis_root_mb }) -name 'root(vis)'))
Write-Host ("  " + (Stat -values @($subsequent | ForEach-Object { $_.vis_web_mb }) -name 'web(vis)'))
Write-Host ("  " + (Stat -values @($subsequent | ForEach-Object { $_.hid_total_mb }) -name 'mem(hid)'))
Write-Host ""
Write-Host ("  exe size (no WebView2 runtime): {0:N2} MB" -f ($exeSize / 1MB))

$summary = [pscustomobject]@{
    runs         = $Runs
    exe_bytes    = $exeSize
    exe_mb       = [Math]::Round($exeSize / 1MB, 2)
    rows         = $rows
}
$summaryPath = Join-Path $OutDir 's3-phaseA-summary.json'
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $summaryPath -Encoding UTF8
Write-Host "summary json : $summaryPath"
