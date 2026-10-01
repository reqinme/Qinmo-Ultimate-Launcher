# S2 analysis: did `backdrop-filter` actually blur anything?
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# THE MEASUREMENT, AND WHY IT IS THIS ONE
#
# The test page draws a high-frequency RED/BLACK checkerboard. A real Gaussian
# blur of radius R lowers the local red-channel contrast inside the card while
# leaving it intact in the control card (identical styling, no backdrop-filter).
#
#   R = mean |red(x+1,y) - red(x,y)| over the crop, red channel only
#
# Red is used because it survives every material blend nearly untouched. An
# earlier round of this project measured SATURATION and concluded "washed out"
# for every channel -- a measurement artifact, not a finding.
#
# VERDICT RULES (decided BEFORE looking at the numbers, so the numbers cannot
# be read to fit a conclusion):
#
#   blurred   : card_R / control_R <= 0.5
#   not blurred: card_R / control_R >= 0.8
#   between   : partial / inconclusive -> report as such, do not round to a side
#
# The control column is what makes this falsifiable: if BOTH crops come out
# smooth, something other than `backdrop-filter` is doing the smoothing and the
# cell is marked INVALID rather than "blurred".
#
# Usage:
#   powershell -File spikes/s1-material/analyze-s2-blur.ps1
# ---------------------------------------------------------------------------

param(
    [string]$OutDir = ""
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$spikeDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $spikeDir 'out' }
$OutDir = (Resolve-Path $OutDir).Path

$summaryPath = Join-Path $OutDir 's2-summary.json'
if (-not (Test-Path $summaryPath)) { throw "missing $summaryPath - run capture-s2-blur.ps1 first" }
$cells = Get-Content $summaryPath -Raw -Encoding UTF8 | ConvertFrom-Json

# Local horizontal contrast on LUMINANCE. Skips a 4px border so the card's own
# 1px outline and rounded corners do not contribute.
#
# WHY LUMINANCE AND NOT THE RED CHANNEL:
# the first version of this script measured the red channel, on the assumption
# that the test page drew a red/black checkerboard. It did not: the CSS rendered
# WHITE and near-BLACK (measured R255 vs R16), and a red-channel metric on a
# greyscale pattern is an accident that happens to work. The page is now a real
# red/black checkerboard, but luminance is the right metric for BOTH patterns --
# it measures "how sharp is the edge", which is exactly what blur destroys.
function Measure-RedContrast {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path $Path)) { return $null }
    $bmp = [System.Drawing.Bitmap]::FromFile($Path)
    try {
        $inset = 4
        $x0 = [Math]::Max(0, $inset)
        $y0 = [Math]::Max(0, $inset)
        $x1 = $bmp.Width - $inset - 1
        $y1 = $bmp.Height - $inset - 1
        if ($x1 -le $x0 -or $y1 -le $y0) { return $null }

        $sum = 0.0
        $n = 0
        for ($y = $y0; $y -le $y1; $y++) {
            for ($x = $x0; $x -lt $x1; $x++) {
                $pa = $bmp.GetPixel($x, $y)
                $pb = $bmp.GetPixel($x + 1, $y)
                $la = 0.299 * $pa.R + 0.587 * $pa.G + 0.114 * $pa.B
                $lb = 0.299 * $pb.R + 0.587 * $pb.G + 0.114 * $pb.B
                $sum += [Math]::Abs($la - $lb)
                $n++
            }
        }
        if ($n -eq 0) { return $null }
        return [pscustomobject]@{ mean = ($sum / $n); samples = $n }
    }
    finally { $bmp.Dispose() }
}

$rows = @()
foreach ($c in $cells) {
    $card = Measure-RedContrast -Path $c.card_crop
    $ctrl = Measure-RedContrast -Path $c.control_crop

    $ratio = $null
    $verdict = 'no-data'
    if ($card -and $ctrl -and $ctrl.mean -gt 0.0001) {
        $ratio = $card.mean / $ctrl.mean
        if ($ratio -ge 0.8) { $verdict = 'NO-BLUR' }
        elseif ($ratio -le 0.5) { $verdict = 'BLURRED' }
        else { $verdict = 'PARTIAL' }
        # Both smooth => the control is not a valid control for this cell.
        if ($ctrl.mean -lt 5.0 -and $card.mean -lt 5.0) { $verdict = 'INVALID(control-also-smooth)' }
    }

    $rows += [pscustomobject]@{
        id         = $c.id
        transparent = $c.transparent
        blur_flag  = $c.blur
        material   = $c.material
        card_R     = if ($card) { [Math]::Round($card.mean, 2) } else { $null }
        control_R  = if ($ctrl) { [Math]::Round($ctrl.mean, 2) } else { $null }
        ratio      = if ($null -ne $ratio) { [Math]::Round($ratio, 3) } else { $null }
        verdict    = $verdict
    }
}

Write-Host ""
Write-Host "==== S2: does backdrop-filter blur? ====" -ForegroundColor Cyan
$rows | Format-Table -AutoSize id, transparent, blur_flag, material, card_R, control_R, ratio, verdict |
    Out-String -Width 240 | Write-Host

# -- the two questions, answered only if the relevant cells exist -------------
function Get-Row { param([string]$Id) return ($rows | Where-Object { $_.id -eq $Id } | Select-Object -First 1) }

Write-Host "---- conclusions ----" -ForegroundColor Cyan
$ob = Get-Row 'opaque-blur'
$on = Get-Row 'opaque-noblur'
$tb = Get-Row 'trans-blur'
$tn = Get-Row 'trans-noblur'

if ($on -and $on.verdict -eq 'NO-BLUR' -and $ob) {
    Write-Host ("  (A) OPAQUE window (the design we ship): backdrop-filter => {0}" -f $ob.verdict)
} else { Write-Host "  (A) opaque: control cell missing or itself blurred - cannot conclude" -ForegroundColor Yellow }

if ($tn -and $tn.verdict -eq 'NO-BLUR' -and $tb) {
    Write-Host ("  (B) TRANSPARENT window: backdrop-filter => {0}" -f $tb.verdict)
    if ($ob -and $tb) {
        if ($ob.verdict -eq 'BLURRED' -and $tb.verdict -eq 'NO-BLUR') {
            Write-Host "  ==> transparent window BREAKS backdrop-filter (platform defect confirmed)" -ForegroundColor Yellow
        } elseif ($ob.verdict -eq 'BLURRED' -and $tb.verdict -eq 'BLURRED') {
            Write-Host "  ==> they COEXIST (assumption was wrong)" -ForegroundColor Green
        } elseif ($ob.verdict -eq 'BLURRED' -and $tb.verdict -eq 'PARTIAL') {
            Write-Host "  ==> coexists but transparent side is DEGRADED" -ForegroundColor Yellow
        }
    }
} else { Write-Host "  (B) transparent: control cell missing or itself blurred - cannot conclude" -ForegroundColor Yellow }

$out = Join-Path $OutDir 's2-analysis.json'
$rows | ConvertTo-Json -Depth 5 | Set-Content -Path $out -Encoding UTF8
Write-Host ""
Write-Host "analysis json: $out"
