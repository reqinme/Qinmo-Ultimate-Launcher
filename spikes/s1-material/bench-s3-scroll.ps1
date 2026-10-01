# S3 item 5: scrolling frame rate on integrated graphics.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# WHAT IS MEASURED
#
# The page builds a long list WITH IMAGES and scrolls itself, timing each frame
# with requestAnimationFrame. The spike prints one machine-readable line:
#
#     SCROLL_RESULT {"frames":..,"mean_ms":..,"p50_ms":..,"p95_ms":..,...}
#
# This script runs that a few times, parses those lines, and compares against the
# budget row "must not drop below 60 fps".
#
# WHY THE PAGE SCROLLS ITSELF
#
# Driving the scroll from here (SendInput / window messages) would mix input
# injection jitter into the frame timing. A measurement whose noise source is the
# measurement tool is the failure mode this project keeps hitting, so the page
# owns both the scrolling and the timing.
#
# HOW 60 FPS IS DECIDED -- and why not from the mean
#
# A 60 Hz compositor gives ~16.67 ms between frames when nothing is dropped. The
# row that matters is "does not drop below 60 fps", so the deciding number is the
# p95 frame interval (and the worst case, reported but not decisive): a good mean
# can hide periodic stutter, and periodic stutter is exactly what a user sees.
#
#   PASS : p95_ms <= 17.0
#   FAIL : p95_ms > 17.0
#
# The threshold is 17.0 rather than 16.67 to allow measurement jitter without
# allowing a genuinely dropped frame; a dropped frame lands at ~33 ms, far above
# either number, so the exact cut point is not load-bearing.
#
# Usage:
#   powershell -File spikes/s1-material/bench-s3-scroll.ps1
#   powershell -File spikes/s1-material/bench-s3-scroll.ps1 -Runs 5 -ScrollMs 8000
# ---------------------------------------------------------------------------

param(
    [int]$Runs = 3,
    [int]$ScrollMs = 6000,
    [int]$Items = 240,
    [string]$OutDir = '',
    [string]$Material = 'none'
)

$ErrorActionPreference = 'Stop'

# The page publishes its result in the WINDOW TITLE (see the page comment for
# why not stdout). So the harness needs GetWindowText.
$titleSrc = @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WinTitle {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextLengthW(IntPtr hWnd);
    public static string Get(IntPtr h) {
        int len = GetWindowTextLengthW(h);
        if (len <= 0) return "";
        var sb = new StringBuilder(len + 2);
        GetWindowTextW(h, sb, sb.Capacity);
        return sb.ToString();
    }
    public static IntPtr FindByPid(int pid) {
        return IntPtr.Zero;
    }
}
"@
Add-Type -TypeDefinition $titleSrc

$spikeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeDir 'out\s3-scroll' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$exe = Join-Path $spikeDir 'target\release\s1-material.exe'
if (-not (Test-Path $exe)) { throw "release exe not found; run 'cargo build --release' in $spikeDir first" }
Write-Output ("exe  : {0}" -f $exe)
Write-Output ("conf : runs={0} scrollMs={1} items={2} material={3}" -f $Runs, $ScrollMs, $Items, $Material)

$rows = @()
for ($i = 1; $i -le $Runs; $i++) {
    Write-Output ""
    Write-Output ("-- run {0}/{1} --" -f $i, $Runs)

    $resultPath = Join-Path $OutDir ("result-scroll-{0:D2}.json" -f $i)
    $stdoutPath = Join-Path $OutDir ("stdout-scroll-{0:D2}.txt" -f $i)
    if (Test-Path $resultPath) { Remove-Item $resultPath -Force }

        # --exit-after-ms makes the app quit by itself. Closing the last window is
    # NOT enough: a Tauri app keeps running by default, which is why the first
    # attempt here reported a timeout on every run.
    $exitAfter = $ScrollMs + 1500
    $argv = @('--material', $Material, '--scroll-test',
        '--scroll-ms', "$ScrollMs", '--scroll-items', "$Items",
        '--exit-after-ms', "$exitAfter",
        '--result', $resultPath)

    # Paths contain spaces in this repo, and Start-Process does NOT quote array
    # elements. Without this the arguments are split mid-path -- a bug already
    # paid for once in this project (see S2's write-up).
    $quoted = @()
    foreach ($a in $argv) { if ($a -match '\s') { $quoted += '"' + $a + '"' } else { $quoted += $a } }

    # stdout must be REDIRECTED, not piped: the page logs SCROLL_RESULT with
    # console.log, and in a GUI subprocess that text only reaches a redirected
    # stdout. Piping through PowerShell would also capture our own noise.
    $proc = Start-Process -FilePath $exe -ArgumentList $quoted -PassThru `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError (Join-Path $OutDir ("stderr-scroll-{0:D2}.txt" -f $i))

    # Wait for the timed exit. NOTE: the result is CAPTURED, not discarded --
    # an earlier revision wrote `$null = $proc.WaitForExit(...)` while a later
    # line still tested `$exited`. The undefined variable made the timeout branch
    # fire on EVERY run, which read as "the spike hangs" while the process had in
    # fact exited normally. One wrong assignment produced a completely misleading
    # diagnosis.
    $exited = $proc.WaitForExit(($ScrollMs + 30000))

    # The result is POSTED by the page and written by the host to <result>.scroll.
    # Earlier channels (console.log, window title) both failed under WebView2 --
    # see the page's own comment. Read the file, then fall back to the title.
    $sinkPath = "$resultPath.scroll"
    $line = $null
    if (Test-Path $sinkPath) {
        $line = (Get-Content -LiteralPath $sinkPath -Encoding UTF8 -ErrorAction SilentlyContinue |
            Where-Object { $_ -match 'SCROLL_RESULT' } | Select-Object -Last 1)
    }

    # The result lives in the window title. Two ways to read it, in this order:
    #   1. poll while the process is alive (the page sets the title, then the app
    #      keeps running for another moment before the timed exit)
    #   2. ask the process object afterwards -- a process that has already exited
    #      can still report its MainWindowTitle on Windows
    # Reading it only one way was the previous single point of failure.
    if (-not $line) {
    $deadline = (Get-Date).AddSeconds(4)
    while ((Get-Date) -lt $deadline -and -not $line) {
        if ($proc.HasExited) { break }
        try {
            $proc.Refresh()
            $t = [WinTitle]::Get($proc.MainWindowHandle)
            if ($t -and $t -match 'SCROLL_RESULT') { $line = $t }
        }
        catch { }
        if (-not $line) { Start-Sleep -Milliseconds 120 }
    }
    }
    if (-not $line) {
        try {
            $proc.Refresh()
            if ($proc.MainWindowTitle -match 'SCROLL_RESULT') { $line = $proc.MainWindowTitle }
        }
        catch { }
    }
    if (-not $exited) {
        Write-Output "  process still running after the wait window; killing it"
        try { $proc.Kill(); $proc.WaitForExit(3000) | Out-Null } catch { }
    }
    if (-not $line) {
        Write-Output "  NO RESULT: the page did not report SCROLL_RESULT (see stdout-scroll file)"
        $rows += [pscustomobject]@{
            run = $i; frames = $null; mean_ms = $null; p50_ms = $null; p95_ms = $null
            worst_ms = $null; fps_mean = $null; fps_p95 = $null; verdict = 'NO-DATA'
        }
        continue
    }

    $json = $line.Substring($line.IndexOf('{'))
    $m = $json | ConvertFrom-Json
    $verdict = if ($m.p95_ms -le 17.0) { 'PASS' } else { 'FAIL' }
    Write-Output ("  frames={0}  mean={1} ms  p50={2}  p95={3}  worst={4}  => {5} fps mean / {6} fps p95  [{7}]" -f `
            $m.frames, $m.mean_ms, $m.p50_ms, $m.p95_ms, $m.worst_ms, $m.fps_from_mean, $m.fps_from_p95, $verdict)

    $rows += [pscustomobject]@{
        run = $i
        frames = $m.frames
        mean_ms = $m.mean_ms
        p50_ms = $m.p50_ms
        p95_ms = $m.p95_ms
        worst_ms = $m.worst_ms
        fps_mean = $m.fps_from_mean
        fps_p95 = $m.fps_from_p95
        verdict = $verdict
    }
    Start-Sleep -Milliseconds 900
}

$good = @($rows | Where-Object { $_.verdict -ne 'NO-DATA' })
Write-Output ""
Write-Output "==== S3 item 5: scrolling frame rate ===="
if ($good.Count -eq 0) {
    Write-Output "  no usable samples"
}
else {
    $p95 = @($good | ForEach-Object { $_.p95_ms }) | Sort-Object
    $median = if ($p95.Count % 2 -eq 1) { $p95[[int]($p95.Count / 2)] } else { ($p95[$p95.Count / 2 - 1] + $p95[$p95.Count / 2]) / 2 }
    Write-Output ("  samples={0}  p95(frame interval) median={1:N2} ms  worst-of-worst={2:N2} ms" -f `
            $good.Count, $median, (@($good | ForEach-Object { $_.worst_ms }) | Measure-Object -Maximum).Maximum)
    Write-Output ("  budget row: 'must not drop below 60 fps'  ->  PASS if p95(frame interval) <= 17.0 ms")
    $fails = @($good | Where-Object { $_.verdict -eq 'FAIL' })
    if ($fails.Count -eq 0) {
        Write-Output "  VERDICT: PASS (all runs held 60 fps at p95)"
    }
    else {
        Write-Output ("  VERDICT: FAIL ({0} of {1} runs dropped frames at p95)" -f $fails.Count, $good.Count)
    }
}

$summary = [pscustomobject]@{
    generated_at = (Get-Date).ToString('s')
    runs         = $Runs
    scroll_ms    = $ScrollMs
    items        = $Items
    material     = $Material
    threshold_ms = 17.0
    rows         = $rows
}
$jsonPath = Join-Path $OutDir 's3-scroll-summary.json'
$summary | ConvertTo-Json -Depth 5 | Set-Content -Path $jsonPath -Encoding UTF8
Write-Output ("summary: {0}" -f $jsonPath)
