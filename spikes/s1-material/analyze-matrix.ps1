# S1 matrix: pixel analysis of the captured screenshots.
#
# Purpose: decide objectively whether a DWM material produced an observable
# change, instead of trusting "it looks about right" on a photo.
#
# Two questions per screenshot:
#   1. Is the window interior distinct from the CONTROL cell (no material)?
#      -> if Mica/Acrylic interiour == control interiour, the material had no
#         visible effect even though apply_* returned Ok.
#   2. Does the 10px frame area show desktop bleed-through?
#      -> measured as "variance across the top strip". A real translucent
#         material shows content behind it (high variance); an opaque fill is
#         flat (low variance).
#
# Answers are written to a JSON file so the numbers can be quoted in docs
# without transcription risk.
#
# Usage: powershell -File spikes/s1-material/analyze-matrix.ps1

param(
    [string]$OutDir = ""
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$spikeRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeRoot 'out' }

function Get-PixelStats([System.Drawing.Bitmap]$bmp, [string]$region) {
    $w = $bmp.Width; $h = $bmp.Height
    $x0 = 0; $y0 = 0; $x1 = $w; $y1 = $h
    switch ($region) {
        'center'      { $x0 = [int]($w*0.30); $y0 = [int]($h*0.60); $x1 = [int]($w*0.70); $y1 = [int]($h*0.80) }
        'topstrip'    { $x0 = 0;            $y0 = 14;              $x1 = $w;          $y1 = 40 }
        'leftstrip'   { $x0 = 14;           $y0 = [int]($h*0.3);   $x1 = 40;          $y1 = [int]($h*0.7) }
        'bottomstrip' { $x0 = 0;            $y0 = $h-40;           $x1 = $w;          $y1 = $h-14 }
    }
    $min = 255; $max = 0; $sum = 0.0; $n = 0; $sumL = 0.0
    for ($y = $y0; $y -lt $y1; $y += 3) {
        for ($x = $x0; $x -lt $x1; $x += 3) {
            $c = $bmp.GetPixel($x, $y)
            $l = 0.2126*$c.R + 0.7152*$c.G + 0.0722*$c.B
            if ($l -lt $min) { $min = $l }
            if ($l -gt $max) { $max = $l }
            $sum += $l; $sumL += $l*$l; $n++
        }
    }
    $mean = $sum / $n
    $var  = ($sumL / $n) - ($mean * $mean)
    if ($var -lt 0) { $var = 0 }
    return [pscustomobject]@{
        region = $region
        mean   = [math]::Round($mean, 1)
        stddev = [math]::Round([math]::Sqrt($var), 1)
        range  = [math]::Round($max - $min, 1)
        min    = [math]::Round($min, 1)
        max    = [math]::Round($max, 1)
        samples = $n
    }
}

$rows = @()
$shots = Get-ChildItem $OutDir -Filter 'shot-*.png' |
         Where-Object { $_.Name -notlike '*.printwindow.png' } | Sort-Object Name

foreach ($f in $shots) {
    $bmp = [System.Drawing.Bitmap]::FromFile($f.FullName)
    try {
        $center = Get-PixelStats $bmp 'center'
        $top    = Get-PixelStats $bmp 'topstrip'
        $left   = Get-PixelStats $bmp 'leftstrip'
        $bottom = Get-PixelStats $bmp 'bottomstrip'

        # Interior "flatness": a material shows up as a large uniform tinted
        # area; a control window also looks uniform, so the comparison ACROSS
        # cells is what matters, not the absolute value.
        $rows += [pscustomobject]@{
            cell            = $f.BaseName -replace '^shot-', ''
            center_mean     = $center.mean
            center_stddev   = $center.stddev
            top_mean        = $top.mean
            top_stddev      = $top.stddev
            left_stddev     = $left.stddev
            bottom_stddev   = $bottom.stddev
            # bleed-through score: how much the frame strips vary.
            # High = content visible behind the window = translucency.
            frame_variance  = [math]::Round(($top.stddev + $left.stddev + $bottom.stddev) / 3, 1)
        }
    } finally { $bmp.Dispose() }
}

Write-Host ""
Write-Host "cell                     ctr_mean  ctr_sd  top_mean  top_sd  frame_var"
Write-Host "------------------------------------------------------------------------"
foreach ($r in $rows) {
    "{0,-24} {1,8} {2,7} {3,9} {4,7} {5,10}" -f `
        $r.cell, $r.center_mean, $r.center_stddev, $r.top_mean, $r.top_stddev, $r.frame_variance | Write-Host
}

# Control comparison: everything is measured against 'none-bare-auto'
$control = $rows | Where-Object { $_.cell -eq 'none-bare-auto' } | Select-Object -First 1
if ($control) {
    Write-Host ""
    Write-Host "delta vs CONTROL (none-bare-auto):"
    foreach ($r in $rows) {
        if ($r.cell -eq 'none-bare-auto') { continue }
        $d = [math]::Round($r.center_mean - $control.center_mean, 1)
        Write-Host ("  {0,-24} center_mean {1,7} (delta {2,7})" -f $r.cell, $r.center_mean, $d)
    }
}

$analysis = @{
    generated_at = (Get-Date).ToString('s')
    note = "frame_variance ~ translucency: high means content visible behind the window."
    control_cell = 'none-bare-auto'
    rows = $rows
}
$out = Join-Path $OutDir 'matrix-analysis.json'
$analysis | ConvertTo-Json -Depth 6 | Set-Content -Path $out -Encoding UTF8
Write-Host ""
Write-Host "analysis json : $out"
