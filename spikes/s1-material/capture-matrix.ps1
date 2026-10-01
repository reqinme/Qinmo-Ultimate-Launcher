# S1 material matrix: run each config and capture screenshots.
#
# ---------------------------------------------------------------------------
# NOTE: THIS SCRIPT IS DELIBERATELY ASCII-ONLY.
#
# Why: PowerShell 5.1 decodes a .ps1 WITHOUT a UTF-8 BOM as ANSI. Any non-ASCII
# byte (Chinese comment, Chinese string) then turns into mojibake, which breaks
# here-string terminators and produces misleading parse errors such as
# "using directive must appear before any other statement".
#
# That failure cost several debugging rounds. Keeping this file pure ASCII
# removes the whole class of problem instead of relying on "always re-add the
# BOM after editing". The reasoning that used to live in Chinese comments is
# recorded in SESSION.md instead.
#
# Division of labour (deliberate):
#   - spike (Rust) : opens window, calls the material API, WRITES RESULT JSON
#                    (hwnd / window rect / ok flag / raw error)
#   - this script  : launches each matrix cell -> reads its JSON ->
#                    captures the window -> kills the process
#
# Capturing is NOT done inside Rust on purpose: capture is a *verification
# tool*, not the thing under test. If it lived in the shell and had a bug, it
# would be indistinguishable from "the material does not work".
#
# Usage:
#   powershell -File spikes/s1-material/capture-matrix.ps1
#   powershell -File spikes/s1-material/capture-matrix.ps1 -SkipTabbed
# ---------------------------------------------------------------------------

param(
    [switch]$SkipTabbed,
    [int]$SettleMs = 2500,        # extra wait after the spike wrote its JSON
    [string]$OutDir = ""
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$winCapSource = @"
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public class WinCap {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);

    public static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);
    public static readonly IntPtr HWND_NOTOPMOST = new IntPtr(-2);
    public const uint SWP_NOMOVE = 0x0002, SWP_NOSIZE = 0x0001, SWP_SHOWWINDOW = 0x0040;

    // Let the window draw itself into our memory DC: independent of focus and
    // occlusion. flags=2 is PW_RENDERFULLCONTENT, required for DWM-composited
    // windows.
    public static Bitmap GrabPrintWindow(IntPtr h, int w, int hh) {
        Bitmap bmp = new Bitmap(w, hh);
        using (Graphics g = Graphics.FromImage(bmp)) {
            IntPtr hdc = g.GetHdc();
            PrintWindow(h, hdc, 2);
            g.ReleaseHdc(hdc);
        }
        return bmp;
    }

    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
}
"@
Add-Type -TypeDefinition $winCapSource -ReferencedAssemblies 'System.Drawing'

$spikeRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $spikeRoot 'target\debug\s1-material.exe'
if (-not (Test-Path $exe)) { $exe = Join-Path $spikeRoot 'target\release\s1-material.exe' }
if (-not (Test-Path $exe)) {
    throw "spike exe not found. Build first: cargo build --manifest-path `"$spikeRoot\Cargo.toml`""
}

if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeRoot 'out' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# ---------------------------------------------------------------------------
# Matrix definition.
#   core 4 : {Mica, Acrylic} x {decorated, bare}   <- the decision matrix
#   extras : Tabbed bare (third material), Mica dark/light (theme axis),
#            none bare (CONTROL: no material API called at all)
#
# NOTE: every cell MUST end with '--result'. The script appends the quoted JSON
# path after these args; without the flag the spike falls back to its default
# output name and every cell reports "no JSON". That mistake was made once.
#
# The control cell matters: without it we cannot distinguish "the material
# worked" from "the window just looks like that anyway".
# ---------------------------------------------------------------------------
$cells = @(
    @{ id = 'mica-bare-auto';    args = @('--material', 'mica', '--result') },
    @{ id = 'mica-deco-auto';    args = @('--material', 'mica', '--decorations', '--result') },
    @{ id = 'acrylic-bare-auto'; args = @('--material', 'acrylic', '--result') },
    @{ id = 'acrylic-deco-auto'; args = @('--material', 'acrylic', '--decorations', '--result') },
    @{ id = 'mica-bare-dark';    args = @('--material', 'mica', '--dark', '--result') },
    @{ id = 'mica-bare-light';   args = @('--material', 'mica', '--light', '--result') },
    @{ id = 'none-bare-auto';    args = @('--material', 'none', '--result') }
)
if (-not $SkipTabbed) {
    $cells += @{ id = 'tabbed-bare-auto'; args = @('--material', 'tabbed', '--result') }
}

$summary = @()

foreach ($cell in $cells) {
    $id = $cell.id
    $jsonPath = Join-Path $OutDir "result-$id.json"
    $pngPath = Join-Path $OutDir "shot-$id.png"
    $pwPath = Join-Path $OutDir "shot-$id.printwindow.png"
    Remove-Item $jsonPath, $pngPath, $pwPath -Force -ErrorAction SilentlyContinue

    Write-Host ""
    Write-Host "== [$id] ==" -ForegroundColor Cyan

    # The result path MUST be quoted as a whole. Passing an array to
    # -ArgumentList splits paths containing spaces (this repo path contains
    # "DeepSeek desktop"), so Rust would receive a truncated --result and the
    # JSON write would fail silently. That trap cost one debugging round.
    $argLine = ($cell.args -join ' ') + ' "' + $jsonPath + '"'
    $proc = Start-Process -FilePath $exe -ArgumentList $argLine -PassThru

    try {
        $deadline = (Get-Date).AddSeconds(25)
        while (-not (Test-Path $jsonPath) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 200
        }
        if (-not (Test-Path $jsonPath)) {
            Write-Host "  FAIL: no result JSON within 25s" -ForegroundColor Red
            $summary += [pscustomobject]@{ id = $id; apply_ok = '(timeout)'; error = ''; png = ''; note = 'no JSON' }
            continue
        }

        $r = Get-Content $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
        Write-Host ("  api={0} dark={1} deco={2} apply_ok={3}" -f `
            $r.spec.api_called, $r.spec.dark_param, $r.spec.decorations, $r.apply_ok)
        if ($r.apply_error) { Write-Host ("  apply_error = {0}" -f $r.apply_error) -ForegroundColor Yellow }

        Start-Sleep -Milliseconds $SettleMs

        $h = [IntPtr][int64]$r.hwnd
        $x = [int]$r.window_x - 12
        $y = [int]$r.window_y - 12
        $w = [int]$r.window_width + 24
        $hh = [int]$r.window_height + 24
        if ($w -le 0 -or $hh -le 0) { $x = 0; $y = 0; $w = 1200; $hh = 860 }

        # -- method 1: PrintWindow (focus-independent, but may not show the
        #    desktop bleed-through of a DWM material) ------------------------
        try {
            [WinCap]::ShowWindow($h, 5) | Out-Null
            Start-Sleep -Milliseconds 400
            $pw = [WinCap]::GrabPrintWindow($h, $r.window_width, $r.window_height)
            $pw.Save($pwPath, [System.Drawing.Imaging.ImageFormat]::Png)
            $pw.Dispose()
            Write-Host ("  PrintWindow {0} KB" -f [math]::Round((Get-Item $pwPath).Length / 1KB, 1)) -ForegroundColor Green
        } catch {
            Write-Host "  PrintWindow failed: $($_.Exception.Message)" -ForegroundColor Yellow
        }

        # -- method 2: topmost + screen capture (shows material bleed-through,
        #    but requires the window to actually be in front) ----------------
        try {
            [WinCap]::SetWindowPos($h, [WinCap]::HWND_TOPMOST, 0, 0, 0, 0,
                [WinCap]::SWP_NOMOVE -bor [WinCap]::SWP_NOSIZE -bor [WinCap]::SWP_SHOWWINDOW) | Out-Null
            [WinCap]::BringWindowToTop($h) | Out-Null
            [WinCap]::SetForegroundWindow($h) | Out-Null
            Start-Sleep -Milliseconds 900

            $bmp = New-Object System.Drawing.Bitmap($w, $hh)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($w, $hh)))
            $bmp.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)
            $g.Dispose(); $bmp.Dispose()

            [WinCap]::SetWindowPos($h, [WinCap]::HWND_NOTOPMOST, 0, 0, 0, 0,
                [WinCap]::SWP_NOMOVE -bor [WinCap]::SWP_NOSIZE) | Out-Null
            Write-Host ("  Screen {0} KB" -f [math]::Round((Get-Item $pngPath).Length / 1KB, 1)) -ForegroundColor Green
        } catch {
            Write-Host "  Screen capture failed: $($_.Exception.Message)" -ForegroundColor Yellow
        }

        $summary += [pscustomobject]@{
            id       = $id
            apply_ok = $r.apply_ok
            error    = $r.apply_error
            png      = $pngPath
            note     = ("{0}x{1} deco={2} dark={3}" -f $r.window_width, $r.window_height, $r.spec.decorations, $r.spec.dark_param)
        }
    }
    finally {
        if ($proc -and -not $proc.HasExited) {
            $proc.Kill(); $proc.WaitForExit(3000) | Out-Null
        }
    }
}

Write-Host ""
Write-Host "==== summary ====" -ForegroundColor Cyan
$summary | Format-Table -AutoSize | Out-String -Width 240 | Write-Host

$summaryPath = Join-Path $OutDir 'matrix-summary.json'
$summary | ConvertTo-Json -Depth 5 | Set-Content -Path $summaryPath -Encoding UTF8
Write-Host "summary json : $summaryPath"
Write-Host "shots dir    : $OutDir"
