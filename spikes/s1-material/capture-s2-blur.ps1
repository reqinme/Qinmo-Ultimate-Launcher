# S2 spike: transparent window x CSS backdrop-filter matrix.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY.
#
# Why: PowerShell 5.1 decodes a .ps1 WITHOUT a UTF-8 BOM as ANSI. Any non-ASCII
# byte (Chinese comment or string) turns into mojibake, which breaks here-string
# terminators and produces misleading parse errors. That cost several rounds.
# Reasoning that used to live in Chinese comments is in SESSION.md instead.
#
# WHAT THIS CAPTURES, AND WHY IN THIS ORDER
#
# The question is NOT "does the card look glassy". It is:
#   (A) does `backdrop-filter` work in an OPAQUE window?   <- the design we ship
#   (B) does a TRANSPARENT window break it?                <- the original worry
#
# So the matrix varies BOTH transparency and blur:
#
#   id                    transparent  material  blur
#   opaque-noblur         no           none      no     <- control (no blur at all)
#   opaque-blur           no           none      yes    <- (A) the shipped design
#   trans-noblur          yes          none      no     <- control for (B)
#   trans-blur            yes          none      yes    <- (B) the worry
#
# Material is held OFF for all four cells on purpose: DWM material composites
# at the WINDOW level and would tint the sample, making it harder to tell
# "smoothed by CSS" from "tinted by DWM". Material gets its own pair at the end.
#
# Capturing is NOT done inside Rust on purpose: capture is a verification tool,
# not the thing under test. A bug in capture must not look like a product bug.
#
# Usage:
#   powershell -File spikes/s1-material/capture-s2-blur.ps1
#   powershell -File spikes/s1-material/capture-s2-blur.ps1 -SettleMs 3000
# ---------------------------------------------------------------------------

param(
    [int]$SettleMs = 2500,
    [string]$OutDir = "",
    [switch]$SkipAfterMaterial
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$src = @"
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public class S2Cap {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    public static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);
    public static readonly IntPtr HWND_NOTOPMOST = new IntPtr(-2);
    public const uint SWP_NOMOVE = 0x0002, SWP_NOSIZE = 0x0001, SWP_SHOWWINDOW = 0x0040;
    public static void ToTop(IntPtr h) {
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        BringWindowToTop(h);
        SetForegroundWindow(h);
    }
    public static void NotTop(IntPtr h) {
        SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
    }
}
"@
Add-Type -TypeDefinition $src

$spikeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeDir 'out' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$exe = Join-Path $spikeDir 'target\debug\s1-material.exe'
if (-not (Test-Path $exe)) {
    # Release is preferred (closer to shipping) but debug is fine here: this
    # spike measures BLUR, not performance. Fall back rather than fail.
    $exe = Join-Path $spikeDir 'target\release\s1-material.exe'
}
if (-not (Test-Path $exe)) { throw "spike exe not found; run 'cargo build' in $spikeDir first" }
Write-Host "exe : $exe" -ForegroundColor Cyan

# Crop region: the window rect reported by the spike, plus a 12px margin so the
# transparent edge is visible. The card rect from the JSON is then offset by the
# same 12px, which is why the margin is a named constant and not a magic number.
$MARGIN = 12

$cells = @(
    @{ id = 'opaque-noblur'; transparent = $false; blur = $false; material = 'none' },
    @{ id = 'opaque-blur';   transparent = $false; blur = $true;  material = 'none' },
    @{ id = 'trans-noblur';  transparent = $true;  blur = $false; material = 'none' },
    @{ id = 'trans-blur';    transparent = $true;  blur = $true;  material = 'none' }
)
if (-not $SkipAfterMaterial) {
    # Material increment cells: same page (blur off), DWM material ON, to see
    # whether material changes the sample enough to confound the blur reading.
    $cells += @(
        @{ id = 'opaque-mica';   transparent = $false; blur = $false; material = 'mica' },
        @{ id = 'trans-mica';    transparent = $true;  blur = $false; material = 'mica' }
    )
}

function Invoke-Cell {
    param($cell)

    $id = $cell.id
    $resultPath = Join-Path $OutDir "result-$id.json"
    if (Test-Path $resultPath) { Remove-Item $resultPath -Force }

    $argv = @('--material', $cell.material, '--result', $resultPath)
    if ($cell.transparent) { $argv += '--transparent' }
    if ($cell.blur) { $argv += '--blur' }

    # ⚠️ `Start-Process -ArgumentList` joins the array with spaces and does NOT
    # quote elements. Our paths contain spaces ("DeepSeek desktop"), so an
    # unquoted `--result C:\...\DeepSeek desktop\...` reaches the spike as TWO
    # arguments and the path is silently truncated at "DeepSeek". The spike then
    # writes its JSON somewhere else and this script waits until it times out.
    #
    # That is exactly the "verification condition does not match the real
    # condition" failure this project has hit before, so it is fixed here once,
    # for every argument, instead of being avoided by keeping paths space-free.
    $quoted = @()
    foreach ($a in $argv) {
        if ($a -match '\s') { $quoted += '"' + $a + '"' } else { $quoted += $a }
    }

    Write-Host ""
    Write-Host ("==== {0} ====  argv: {1}" -f $id, ($quoted -join ' ')) -ForegroundColor Cyan

    $proc = Start-Process -FilePath $exe -ArgumentList $quoted -PassThru
    try {
        # Wait for the spike to write its JSON (that is its "I am ready" signal).
        $deadline = (Get-Date).AddSeconds(25)
        while (-not (Test-Path $resultPath)) {
            if ($proc.HasExited) { throw "spike exited early (code $($proc.ExitCode)) before writing JSON" }
            if ((Get-Date) -gt $deadline) { throw "timed out waiting for $resultPath" }
            Start-Sleep -Milliseconds 200
        }
        Start-Sleep -Milliseconds 400   # let the file finish flushing

        $r = Get-Content $resultPath -Raw -Encoding UTF8 | ConvertFrom-Json
        Write-Host ("  apply_ok={0} transparent={1} blur={2}" -f `
            $r.apply_ok, $r.spec.transparent, $r.spec.blur)

        Start-Sleep -Milliseconds $SettleMs

        $h = [IntPtr][int64]$r.hwnd
        [S2Cap]::ShowWindow($h, 5) | Out-Null
        [S2Cap]::ToTop($h)
        Start-Sleep -Milliseconds 900

        $wx = [int]$r.window_x - $MARGIN
        $wy = [int]$r.window_y - $MARGIN
        $ww = [int]$r.window_width + (2 * $MARGIN)
        $wh = [int]$r.window_height + (2 * $MARGIN)
        if ($ww -le 0 -or $wh -le 0) { throw "bad window rect from JSON" }

        $bmp = New-Object System.Drawing.Bitmap($ww, $wh)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($wx, $wy, 0, 0, (New-Object System.Drawing.Size($ww, $wh)))
        $g.Dispose()

        $fullPath = Join-Path $OutDir "shot-$id.png"
        $bmp.Save($fullPath, [System.Drawing.Imaging.ImageFormat]::Png)
        Write-Host ("  full  {0} KB" -f [math]::Round((Get-Item $fullPath).Length / 1KB, 1)) -ForegroundColor Green

        # -- card crops ------------------------------------------------------
        # Page origin maps to (MARGIN, MARGIN) inside the captured bitmap.
        $crop = @{}
        foreach ($which in @('card', 'control')) {
            if ($which -eq 'card') { $rect = $r.spec.card_rect } else { $rect = $r.spec.control_rect }
            $cx = [int]([math]::Round($rect.x)) + $MARGIN
            $cy = [int]([math]::Round($rect.y)) + $MARGIN
            $cw = [int]([math]::Round($rect.w))
            $ch = [int]([math]::Round($rect.h))
            if ($cx + $cw -gt $ww -or $cy + $ch -gt $wh) {
                Write-Host "  WARN: $which rect falls outside the captured window; skipping crop" -ForegroundColor Yellow
                continue
            }
            $dst = New-Object System.Drawing.Rectangle($cx, $cy, $cw, $ch)
            $piece = $bmp.Clone($dst, $bmp.PixelFormat)
            $cropPath = Join-Path $OutDir "crop-$id-$which.png"
            $piece.Save($cropPath, [System.Drawing.Imaging.ImageFormat]::Png)
            $piece.Dispose()
            $crop[$which] = $cropPath
        }
        $bmp.Dispose()

        [S2Cap]::NotTop($h) | Out-Null

        $meta = [pscustomobject]@{
            id           = $id
            transparent  = $r.spec.transparent
            blur         = $r.spec.blur
            material     = $r.spec.material
            apply_ok     = $r.apply_ok
            apply_error  = $r.apply_error
            shot         = $fullPath
            card_crop    = $crop['card']
            control_crop = $crop['control']
            card_rect    = $r.spec.card_rect
            control_rect = $r.spec.control_rect
        }
        return $meta
    }
    finally {
        if ($proc -and -not $proc.HasExited) {
            $proc.Kill(); $proc.WaitForExit(3000) | Out-Null
        }
    }
}

$summary = @()
foreach ($c in $cells) { $summary += Invoke-Cell -cell $c }

Write-Host ""
Write-Host "==== summary ====" -ForegroundColor Cyan
$summary | Format-Table -AutoSize id, transparent, blur, material, apply_ok | Out-String -Width 240 | Write-Host

$summaryPath = Join-Path $OutDir 's2-summary.json'
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $summaryPath -Encoding UTF8
Write-Host "summary json : $summaryPath"
Write-Host "shots dir    : $OutDir"
