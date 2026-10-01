# Guard against accidentally pasting reference-project code or prose into our own sources.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing). Chinese reasoning is
# recorded in the M0 spike report (docs/) instead.
#
# WHY THIS EXISTS
#
# `repos/` holds ~18 reference repositories under MIXED licences, including
# AGPL-3.0 (Portal) and GPL-3.0 (LeviLauncher, HMCL, Prism, ...). Our standing
# rule is "borrow the mechanism, never the code". A rule like that decays
# silently: a long identical line is exactly what a hurried copy-paste looks
# like, and nothing else in CI would notice.
#
# So this check builds a set of long lines from every file under `repos/` and
# fails if any long line in OUR sources matches one.
#
# WHY A LENGTH THRESHOLD, AND WHY 40
#
# Measured before choosing: our own sources had 541 lines of length >= 40, and
# comparing them against 2950 reference .ts/.tsx files produced ZERO matches.
# So 40 is a level at which ordinary code does not collide by accident.
# Short lines are skipped on purpose -- `}`, `use std::fs;`, `import React from`
# are shared vocabulary, not evidence of copying, and flagging them would train
# people to ignore this check.
#
# COST AND LIMITS (stated so nobody over-trusts it)
#
#   * It catches IDENTICAL long lines. Reformatting, renaming or translating
#     defeats it -- it is a safety net for the careless case, not a plagiarism
#     detector.
#   * It rebuilds the reference line set on every run (repos/ is gitignored, so
#     it cannot be a checked-in data file). On this machine that is acceptable;
#     if it becomes slow, cache the set keyed by a hash of the file list.
#
# Usage:
#   powershell -File tools/check-copy-paste.ps1
#   powershell -File tools/check-copy-paste.ps1 -MinLength 60
# ---------------------------------------------------------------------------

param(
    [int]$MinLength = 40,
    [string[]]$ScanDirs = @('crates', 'web/src', 'src-tauri', 'tools'),
    [string]$ReposDir = 'repos',
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$repos = Join-Path $root $ReposDir

# Extensions we treat as source/prose worth comparing.
$codeExt = @('.rs', '.ts', '.tsx', '.js', '.jsx', '.go', '.cs', '.java', '.py', '.cpp', '.h', '.hpp')
$proseExt = @('.md')
# Prose collides far more easily than code (short shared headings), so prose
# needs a much higher bar before a match means anything.
$proseMinLength = [Math]::Max($MinLength * 2, 80)

$skipPathPattern = 'node_modules|\\dist\\|\\build\\|\\target\\|\\.git\\|\\vendor\\|\\bin\\|\\obj\\'

# Lines that are ordinary shared vocabulary rather than evidence of copying.
# These were chosen from the FIRST run of this check, which reported 14 hits and
# every one of them was a line like `#[derive(Debug, Clone, PartialEq, Eq, ...)]`
# or `import type { ReactElement } from "react";`.
#
# That first run is worth remembering: a guard whose findings are all noise gets
# switched off, and then it protects nothing. So the bar is "rare enough to mean
# something", not "identical".
$boilerplatePatterns = @(
    '^\s*#\[derive\(',
    '^\s*#\[cfg\(',
    '^\s*use\s+[A-Za-z0-9_:]+;?$',
    '^\s*import\s+(type\s+)?[{*]',
    '^\s*export\s+(type\s+)?[{*]',
    '^\s*from\s+["'']',
    '^\s*pub\s+const\s+fn\s+as_str\(',
    '^\s*Self::[A-Za-z0-9_]+\s*=>\s*"[a-z_]+",?$',
    '^\s*let\s+Ok\(entries\)\s*=\s*std::fs::read_dir',
    '^\s*let\s+mut\s+stack\s*=\s*vec!\[',
    '^\s*<[a-zA-Z]+[^>]*>\s*$'
)

function Test-Boilerplate {
    param([string]$Line)
    foreach ($p in $boilerplatePatterns) {
        if ($Line -match $p) { return $true }
    }
    return $false
}

# Returns a hashtable: trimmed line -> [pscustomobject]@{ File = first file seen; Count = files seen in }
#
# COUNT MATTERS. A line appearing in 2+ reference files is shared vocabulary, not
# a copied line: nobody copies the same line from two unrelated projects.
# A line unique to ONE reference file is the interesting case.
function Get-LongLines {
    param([string]$Dir, [string[]]$Extensions, [int]$Min)
    $set = @{}
    if (-not (Test-Path $Dir)) { return $set }
    $files = Get-ChildItem $Dir -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $Extensions -contains $_.Extension.ToLower() -and $_.FullName -notmatch $skipPathPattern }
    foreach ($f in $files) {
        if ($f.Length -gt 2MB) { continue }
        $seenInThisFile = @{}
        foreach ($line in (Get-Content -LiteralPath $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue)) {
            $k = $line.Trim()
            if ($k.Length -lt $Min) { continue }
            if (Test-Boilerplate -Line $k) { continue }
            if ($seenInThisFile.ContainsKey($k)) { continue }
            $seenInThisFile[$k] = $true
            if ($set.ContainsKey($k)) {
                $set[$k].Count = $set[$k].Count + 1
            }
            else {
                $set[$k] = [pscustomobject]@{ File = $f.FullName; Count = 1 }
            }
        }
    }
    return $set
}

Write-Output "root  : $root"
if (-not (Test-Path $repos)) {
    # Absence of repos/ is NOT a pass: it means the check could not run, and
    # silently passing would turn a missing precondition into a green light.
    Write-Output "repos/ not found -- cannot run the copy-paste check."
    Write-Output "This is reported as a failure on purpose: a check that could not run must not look like a pass."
    exit 2
}

Write-Output "building reference line set from $ReposDir ..."
$refCode = Get-LongLines -Dir $repos -Extensions $codeExt -Min $MinLength
$refProse = Get-LongLines -Dir $repos -Extensions $proseExt -Min $proseMinLength
Write-Output ("  code lines >= {0}: {1}" -f $MinLength, $refCode.Count)
Write-Output ("  prose lines >= {0}: {1}" -f $proseMinLength, $refProse.Count)

$violations = @()
foreach ($rel in $ScanDirs) {
    $dir = Join-Path $root $rel
    if (-not (Test-Path $dir)) { continue }
    $files = Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { ($codeExt + $proseExt) -contains $_.Extension.ToLower() -and $_.FullName -notmatch $skipPathPattern }
    foreach ($f in $files) {
        $isProse = $f.Extension.ToLower() -eq '.md'
        $min = if ($isProse) { $proseMinLength } else { $MinLength }
        $ref = if ($isProse) { $refProse } else { $refCode }
        $lineNo = 0
        foreach ($line in (Get-Content -LiteralPath $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue)) {
            $lineNo++
            $k = $line.Trim()
            if ($k.Length -lt $min) { continue }
            if ($ref.ContainsKey($k)) {
                # Only a line that is UNIQUE to one reference file is reported.
                # A line seen in 2+ unrelated projects is shared vocabulary.
                if ($ref[$k].Count -eq 1) {
                    $violations += [pscustomobject]@{
                        file     = ($f.FullName.Substring($root.Length).TrimStart('\', '/'))
                        line     = $lineNo
                        source   = ($ref[$k].File.Substring($root.Length).TrimStart('\', '/'))
                        excerpt  = $k.Substring(0, [Math]::Min(100, $k.Length))
                    }
                }
            }
        }
    }
}

Write-Output ""
if ($violations.Count -eq 0) {
    Write-Output ("OK: no long identical lines between our sources and {0}." -f $ReposDir)
    exit 0
}

Write-Output ("FAIL: {0} long identical line(s) between our sources and {1}." -f $violations.Count, $ReposDir)
Write-Output "Borrowing a MECHANISM is allowed; moving its CODE in is not."
Write-Output "If a line is genuinely unavoidable (e.g. a third-party header), narrow this check or record an exception in docs/source-register."
Write-Output ""
foreach ($v in $violations | Select-Object -First 30) {
    Write-Output ("  {0}:{1}" -f $v.file, $v.line)
    Write-Output ("      also in {0}" -f $v.source)
    Write-Output ("      {0}" -f $v.excerpt)
}
if ($violations.Count -gt 30) { Write-Output ("  ... and {0} more" -f ($violations.Count - 30)) }

if (-not [string]::IsNullOrWhiteSpace($OutDir)) {
    if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
    $violations | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $OutDir 'copy-paste-violations.json') -Encoding UTF8
}

exit 1
