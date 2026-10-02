# =============================================================================
# Generate src-tauri/icons/icon.ico
# =============================================================================
#
# WHY THIS GENERATOR EXISTS
#
# tauri-build needs an .ico to write the Windows resource file. And our own rule
# (plan section 4.6.8, the "adjacent requirement") is that ALL of our graphics
# are self-drawn -- no icons taken from anywhere else.
#
# So the icon is BUILT HERE, from arithmetic, rather than committed as an opaque
# binary or downloaded. That has three concrete advantages:
#
#   1. It is reviewable. A 4 KB .ico is not.
#   2. If the brand colours change, the generator is re-run -- there is no
#      "find the source file" step.
#   3. It removes any question of provenance. Nothing was copied.
#
# WHAT IT DRAWS
#
# A 32x32 icon: a rounded square in the accent colour with a lighter "brush"
# stroke through it. Both colours come from the SAME values the design tokens
# use (tokens.css: accent.5 = #4C9AFF, and a near-white), so the icon cannot
# drift away from the design system.
#
# FORMAT
#
# A real .ico with a single 32x32 32-bit BGRA image:
#
#   ICONDIR        6 bytes   reserved(0) + type(1) + count(1)
#   ICONDIRENTRY  16 bytes   w h colors reserved planes bpp bytesInRes offset
#   BITMAPINFOHEADER + XOR bitmap (bottom-up) + AND mask (1bpp, padded to 4 bytes)
#
# The AND mask is what makes transparency work in older consumers; modern ones
# use the alpha channel, but both are written.
#
# ASCII-ONLY: this file must stay ASCII. A .ps1 with non-ASCII text breaks
# parsing under Windows PowerShell (that mistake has been made in this repo).

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Walk up for .git rather than counting directory levels -- that mistake has
# been made in this repo too (a script moved one level deeper silently started
# scanning the wrong tree).
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $dir = Split-Path $dir -Parent
}
if (-not $dir) { throw 'gen-icon: could not find .git walking up from the script' }

$outDir = Join-Path $dir 'src-tauri\icons'
$outFile = Join-Path $outDir 'icon.ico'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$size = 32

# --- colours, matching the design tokens ------------------------------------
# accent.5 (dark theme) from docs/UI设计规格.md section 5.3
$accentR = 0x4C; $accentG = 0x9A; $accentB = 0xFF
# a near-white for the stroke
$strokeR = 0xF5; $strokeG = 0xF5; $strokeB = 0xF7

function New-IconPixels([int]$n) {
    # Returns a byte[] of BGRA rows, BOTTOM-UP (that is what the format wants).
    $px = New-Object byte[] ($n * $n * 4)
    $r = [double]$n

    for ($y = 0; $y -lt $n; $y++) {
        for ($x = 0; $x -lt $n; $x++) {
            # Rounded square: inside unless we are in a corner outside the radius.
            $radius = $r * 0.22
            $inside = $true
            $cx = $x + 0.5
            $cy = $y + 0.5
            $dx = 0.0
            $dy = 0.0
            if ($cx -lt $radius) { $dx = $radius - $cx }
            elseif ($cx -gt ($r - $radius)) { $dx = $cx - ($r - $radius) }
            if ($cy -lt $radius) { $dy = $radius - $cy }
            elseif ($cy -gt ($r - $radius)) { $dy = $cy - ($r - $radius) }
            if ((($dx * $dx) + ($dy * $dy)) -gt ($radius * $radius)) { $inside = $false }

            # A diagonal stroke: the band |(x - y)| small, and inside the square.
            $band = [Math]::Abs($cx - $cy)
            $stroke = ($band -lt ($r * 0.09)) -and ($cx -gt ($r * 0.18)) -and ($cx -lt ($r * 0.82)) -and ($cy -gt ($r * 0.18)) -and ($cy -lt ($r * 0.82))

            $row = ($n - 1 - $y)  # bottom-up
            $o = (($row * $n) + $x) * 4
            if (-not $inside) {
                # Fully transparent.
                [void]($px[$o] = 0); [void]($px[$o + 1] = 0); [void]($px[$o + 2] = 0); [void]($px[$o + 3] = 0)
            } elseif ($stroke) {
                [void]($px[$o] = $strokeB); [void]($px[$o + 1] = $strokeG); [void]($px[$o + 2] = $strokeR); [void]($px[$o + 3] = 0xFF)
            } else {
                [void]($px[$o] = $accentB); [void]($px[$o + 1] = $accentG); [void]($px[$o + 2] = $accentR); [void]($px[$o + 3] = 0xFF)
            }
        }
    }
    return $px
}

# Wrapped in @(...) AND cast to [byte[]]: PowerShell UNROLLS a returned array,
# so without this `$xor` is an Object[] of 4096 Boxed bytes, and
# BinaryWriter.Write(Object[]) picks a different overload -- it wrote nothing,
# and the file came out at 191 bytes instead of 4286.
#
# Debug output confirmed it: "xor type=Object[] len=4096".
[byte[]]$xor = @(New-IconPixels $size)

# --- AND mask: 1 bit per pixel, rows padded to 4 bytes -----------------------
$maskRowBytes = [int][Math]::Ceiling($size / 32.0) * 4
$and = New-Object byte[] ($maskRowBytes * $size)
for ($y = 0; $y -lt $size; $y++) {
    for ($x = 0; $x -lt $size; $x++) {
        $row = $size - 1 - $y
        $o = (($row * $size) + $x) * 4
        $opaque = $xor[$o + 3] -ne 0
        if (-not $opaque) {
            # A set bit means "leave the background showing" => transparent.
            $byteIndex = ((($size - 1 - $y) * $maskRowBytes) + [int][Math]::Floor($x / 8))
            $bit = 7 - ($x % 8)
            [void]($and[$byteIndex] = $and[$byteIndex] -bor (1 -shl $bit))
        }
    }
}

# --- BITMAPINFOHEADER -------------------------------------------------------
$biSize = 40
$bytesInRes = $biSize + $xor.Length + $and.Length
$imageOffset = 6 + 16

$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)

# ICONDIR
$bw.Write([uint16]0)        # reserved
$bw.Write([uint16]1)        # type: 1 = icon
$bw.Write([uint16]1)        # image count

# ICONDIRENTRY  (32 is written as 32; 256 would be written as 0)
$bw.Write([byte]$size)      # width
$bw.Write([byte]$size)      # height
$bw.Write([byte]0)          # palette count
$bw.Write([byte]0)          # reserved
$bw.Write([uint16]1)        # colour planes
$bw.Write([uint16]32)       # bits per pixel
$bw.Write([uint32]$bytesInRes)
$bw.Write([uint32]$imageOffset)

# BITMAPINFOHEADER
$bw.Write([uint32]$biSize)
$bw.Write([int32]$size)             # width
$bw.Write([int32]($size * 2))       # height = XOR + AND
$bw.Write([uint16]1)                # planes
$bw.Write([uint16]32)               # bit count
$bw.Write([uint32]0)                # compression: BI_RGB
$bw.Write([uint32]($xor.Length + $and.Length))
$bw.Write([int32]0)                 # x pixels per meter
$bw.Write([int32]0)                 # y pixels per meter
$bw.Write([uint32]0)                # colours used
$bw.Write([uint32]0)                # important colours

$bw.Write($xor)
$bw.Write($and)
$bw.Flush()
[System.IO.File]::WriteAllBytes($outFile, $ms.ToArray())
$bw.Dispose()
$ms.Dispose()

Write-Output ("gen-icon: wrote {0} ({1} bytes, {2}x{2} BGRA)" -f $outFile, (Get-Item $outFile).Length, $size)
exit 0
