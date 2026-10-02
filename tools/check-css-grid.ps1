# =============================================================================
# check-css-grid.ps1 -- a collapsed (0-width) grid track needs explicit
#                       placement, or the grid places children by DOM order
# =============================================================================
#
# WHY THIS EXISTS -- the second layout defect, found 2026-10-02 by a human eye
#
# `web/src/routes/Shell.css` collapses its middle column when a page has no
# secondary column:
#
#     .shell:not(:has(.shell__secondary)) {
#       grid-template-columns: var(--shell-rail-w) 0 1fr;
#     }
#
# The three children (.shell__rail / .shell__secondary / .shell__main) carried
# NO `grid-column`, so the grid auto-placed them in DOM order. The titlebar
# (which does say `grid-column: 1 / -1`) took the first row, the rail landed in
# column 1, and the MAIN REGION landed in column 2 -- the 0-width one. The home
# page was squeezed into a single vertical column of one character per line and
# the right half of the window was empty.
#
# Nothing saw it. jsdom has no layout engine, so no vitest case can see it;
# tsc and eslint parse, they do not lay out; check-css-tokens.ps1 only reads
# var(--token); check-material-ladder.ps1 only reads numbered token values;
# impeccable only knows anti-patterns. A grid that collapses a track and then
# relies on DOM order is a layout that is only ever verified BY LOOKING.
#
# WHAT IT ASSERTS
#
#   For every `grid-template-columns` declaration whose track list contains a
#   ZERO-width track (0, 0px, 0fr, 0%, 0rem, ...), the same stylesheet must
#   contain at least one explicit placement -- `grid-column`, `grid-column-start`
#   or `grid-area` -- per collapsed declaration. Explicit placement is the only
#   thing that decides which child goes into which track when a track is gone.
#
# WHAT IT DELIBERATELY DOES NOT DO
#
#   - It does not parse CSS. It counts declarations and placements PER FILE, so
#     it can prove "this file collapses a track and never places anything",
#     not "every child of that grid is placed". That is enough for the defect
#     above (Shell.css had zero placements in the whole file) and cheap enough
#     to keep true.
#   - It ignores `minmax(0, X)`: a zero MINIMUM is the ordinary "do not let the
#     track overflow its content" idiom, not a collapsed track.
#   - It does not read inline styles (`style={{ gridColumn: ... }}` in TSX), so
#     a grid declared in CSS but placed from TSX is not counted. None exists.
#   - NO ALLOW-LIST, same reasoning as check-css-tokens.ps1.
#   - The escape hatch is IN THE CSS, next to the rule it excuses: a comment
#     containing `qul-grid-auto-ok` above the declaration turns the failure into
#     a NOTE. Explicit placement is the default because auto-placement into a
#     collapsed track is what broke the window; a deliberate exception belongs
#     where the next reader will see it.
#   - Comments are stripped before looking for DECLARATIONS (a comment ABOUT
#     grid-template-columns is not a declaration), but the escape marker is
#     looked for in the raw text, because a comment is exactly where it lives.
#
# ASCII-ONLY: this file must stay ASCII. Non-ASCII in a .ps1 breaks parsing
# under Windows PowerShell (that mistake has been made in this repo before).

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Walk up for .git rather than counting directory levels -- that mistake has
# been made in this repo (a script moved one level deeper silently scanned the
# wrong tree).
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $parent = Split-Path $dir -Parent
    if ($parent -eq $dir) { $dir = $null } else { $dir = $parent }
}
if (-not $dir) { throw 'check-css-grid: could not find .git walking up from the script' }

$srcDir = Join-Path $dir 'web\src'
if (-not (Test-Path $srcDir)) { throw "check-css-grid: no web/src at $srcDir" }
$srcPrefixLen = $srcDir.Length + 1

# --- text helpers ------------------------------------------------------------

function Add-LineStarts([System.Collections.Generic.List[int]]$list, [string]$text) {
    $list.Add(0)
    for ($i = 0; $i -lt $text.Length; $i++) {
        if ($text[$i] -eq "`n") { $list.Add($i + 1) }
    }
}

function Get-LineOf([System.Collections.Generic.List[int]]$map, [int]$index) {
    $hit = $map.BinarySearch($index)
    if ($hit -lt 0) { $hit = (-$hit) - 2 }
    if ($hit -lt 0) { $hit = 0 }
    return $hit + 1
}

function Remove-BlockComments([string]$text) {
    # Keep the line count identical so line numbers in the report still point
    # at the right place (see check-css-tokens.ps1).
    return [regex]::Replace($text, '/\*.*?\*/', {
            param($m)
            ($m.Value -split "`n" | ForEach-Object { '' }) -join "`n"
        }, 'Singleline')
}

# Split a track list on whitespace that is OUTSIDE any parentheses, so
# `minmax(0, 1fr)` stays one token (its comma is not a separator either).
function Split-Tracks([string]$value) {
    $out = New-Object 'System.Collections.Generic.List[string]'
    $depth = 0
    $cur = New-Object System.Text.StringBuilder
    foreach ($ch in $value.ToCharArray()) {
        if ($ch -eq '(') { $depth++ }
        elseif ($ch -eq ')') { $depth-- }
        if ([char]::IsWhiteSpace($ch) -and $depth -eq 0) {
            if ($cur.Length -gt 0) { $out.Add($cur.ToString()) | Out-Null; $cur.Clear() | Out-Null }
            continue
        }
        $cur.Append($ch) | Out-Null
    }
    if ($cur.Length -gt 0) { $out.Add($cur.ToString()) | Out-Null }
    return , $out
}

# Index of the first comma at parenthesis depth 0, or -1.
function Find-TopComma([string]$text) {
    $depth = 0
    for ($i = 0; $i -lt $text.Length; $i++) {
        $c = $text[$i]
        if ($c -eq '(') { $depth++ }
        elseif ($c -eq ')') { $depth-- }
        elseif ($c -eq ',' -and $depth -eq 0) { return $i }
    }
    return -1
}

# `repeat(12, minmax(0, 1fr))` and `repeat(auto-fit, 200px)` are one token, but
# a track inside them can still be collapsed (`repeat(3, 0)`), so the body of a
# repeat() is expanded back into tracks.
function Expand-Tracks([string[]]$tracks) {
    $out = New-Object 'System.Collections.Generic.List[string]'
    foreach ($t in $tracks) {
        if ($t.StartsWith('repeat(') -and $t.EndsWith(')')) {
            $inner = $t.Substring(7, $t.Length - 8)
            $comma = Find-TopComma $inner
            if ($comma -gt 0) {
                foreach ($x in (Expand-Tracks (Split-Tracks $inner.Substring($comma + 1)))) {
                    $out.Add($x) | Out-Null
                }
                continue
            }
        }
        $out.Add($t) | Out-Null
    }
    return , $out
}

function Test-ZeroTrack([string]$t) {
    return $t -match '^0(\.0+)?(px|fr|%|rem|em|ch|ex|vw|vh|pt|pc|cm|mm|in)?$'
}

# --- the check ---------------------------------------------------------------

$files = @(Get-ChildItem $srcDir -Recurse -File -Filter '*.css' | Sort-Object FullName)
if ($files.Count -eq 0) { throw "check-css-grid: no .css under $srcDir" }

$declarationCount = 0
$failures = New-Object 'System.Collections.Generic.List[object]'
$notes = New-Object 'System.Collections.Generic.List[string]'

foreach ($file in $files) {
    $rel = $file.FullName.Substring($srcPrefixLen)
    $raw = [System.IO.File]::ReadAllText($file.FullName)
    $clean = Remove-BlockComments $raw
    $map = New-Object 'System.Collections.Generic.List[int]'
    Add-LineStarts $map $clean

    $placements = ([regex]::Matches($clean, '(?<![\w-])grid-(column(-start)?|area)\s*:')).Count

    $collapsed = New-Object 'System.Collections.Generic.List[object]'
    foreach ($m in [regex]::Matches($clean, 'grid-template-columns\s*:\s*([^;{}]*)', 'Singleline')) {
        $declarationCount++
        $value = $m.Groups[1].Value
        $tracks = Expand-Tracks (Split-Tracks $value)
        $zeros = @($tracks | Where-Object { Test-ZeroTrack $_ })
        if ($zeros.Count -eq 0) { continue }

        $line = Get-LineOf $map $m.Index
        $before = $raw.Substring([Math]::Max(0, $m.Index - 800), [Math]::Min(800, $m.Index))
        $excused = $before.Contains('qul-grid-auto-ok')
        if ($excused) {
            $notes.Add("$rel`:$line collides a track to zero width and is excused in place (qul-grid-auto-ok)")
            continue
        }
        $collapsed.Add([pscustomobject]@{
                line  = $line
                value = ($value -replace '\s+', ' ').Trim()
            }) | Out-Null
    }

    if ($collapsed.Count -eq 0) { continue }

    if ($placements -lt $collapsed.Count) {
        $failures.Add([pscustomobject]@{
                rel        = $rel
                placements = $placements
                collapsed  = $collapsed
            }) | Out-Null
    }
}

Write-Host ''
Write-Host ("CSS grid: {0} file(s), {1} grid-template-columns declaration(s)" -f $files.Count, $declarationCount)

if ($notes.Count -gt 0) {
    Write-Host ''
    foreach ($n in $notes) { Write-Host "NOTE  $n" }
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host 'FAIL  a grid track is collapsed to zero width while the children'
    Write-Host '      have no explicit placement in the same stylesheet:'
    Write-Host ''
    Write-Host '      With no `grid-column`, the grid places children in DOM order, so'
    Write-Host '      a child that should have the remaining width lands IN the'
    Write-Host '      collapsed track and gets ~0px. That is not a style choice, it is'
    Write-Host '      a layout that depends on element order -- and nothing about it is'
    Write-Host '      visible to jsdom, tsc, eslint or any token checker.'
    Write-Host ''
    Write-Host '      Fix: give the children `grid-column: 1` / `2` / `3` ... , or put'
    Write-Host '      `qul-grid-auto-ok` in a comment above the declaration and say why'
    Write-Host '      auto-placement is correct here.'
    Write-Host ''
    foreach ($f in $failures) {
        Write-Host ("      {0}  (explicit placements in this file: {1})" -f $f.rel, $f.placements)
        foreach ($c in $f.collapsed) {
            Write-Host ("          line {0}: grid-template-columns: {1}" -f $c.line, $c.value)
        }
    }
    Write-Host ''
    Write-Host 'See the header of this script for the real defect it encodes.'
    exit 1
}

Write-Host 'OK: every collapsed grid track sits in a stylesheet that places its children.'
exit 0
