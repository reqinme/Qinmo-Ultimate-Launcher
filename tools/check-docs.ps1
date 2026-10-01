# check-docs.ps1 -- structural / integrity checks for docs/*.md
#
# ---------------------------------------------------------------------------
# WHY THIS EXISTS
#
# Four times in this project a heading was silently EATEN by an edit: the
# `edit` tool replaces old_string with new_string, so when a heading line was
# used purely as an anchor and not copied into new_string, the heading vanished
# while its body survived and re-attached to the previous heading.
#
# The document stays VALID Markdown, and the checks that existed at the time
# ("duplicate headings == 0", "code fences paired") CANNOT see it:
#   - a heading that disappeared is not a duplicate
#   - fence pairing is unaffected
#
# That is the real root cause: the checks measured "did anything EXTRA appear"
# while the bug was "did something GO MISSING".
#
# So this script adds HEADING CONSERVATION:
#   the total heading count per file must equal the recorded baseline.
# Changing the count is allowed, but only as an EXPLICIT, reviewable act
# (`-UpdateBaseline`), never as a side effect of an unrelated edit.
#
# ---------------------------------------------------------------------------
# NOTE: ASCII-ONLY ON PURPOSE.
# PowerShell 5.1 decodes a .ps1 without a UTF-8 BOM as ANSI. Non-ASCII bytes
# then turn into mojibake and break parsing (here-string terminators, string
# quotes). Keeping this file pure ASCII removes the whole class of problem.
# Chinese filenames are fine -- they are read as data, not as script text.
# ---------------------------------------------------------------------------

param(
    [switch]$UpdateBaseline,

    # Exit non-zero when any check fails. The default is to just report,
    # so a human can run it casually while editing.
    [switch]$Strict
)

$ErrorActionPreference = 'Stop'

# Force UTF-8 for the console so Chinese filenames render correctly on both
# Windows PowerShell 5.1 (which defaults to the ANSI code page) and pwsh 7
# (which defaults to UTF-8). Without this, the CI log is mojibake on 5.1 and
# people stop trusting the tool's output.
try {
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    $OutputEncoding = New-Object System.Text.UTF8Encoding($false)
} catch {
    # Non-fatal: only affects how filenames are echoed.
}

$repoRoot  = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($repoRoot)) { $repoRoot = (Get-Location).Path }
$docsRoot  = Join-Path $repoRoot 'docs'
$baselinePath = Join-Path $docsRoot '.heading-baseline.json'

if (-not (Test-Path $docsRoot)) { throw "docs directory not found: $docsRoot" }

# ---------------------------------------------------------------------------
# Collect the markdown files.
# We only read UTF-8 text: NEVER use Get-Content here, because PowerShell 5.1
# decodes UTF-8-without-BOM as ANSI and would report mojibake filenames.
# ---------------------------------------------------------------------------
$ignoredDirs = @('_artifacts')      # screenshots / mockups: no doc structure
$mdFiles = Get-ChildItem $docsRoot -Recurse -File -Filter '*.md' |
    Where-Object { $rel = $_.FullName.Substring($docsRoot.Length); -not ($ignoredDirs | Where-Object { $rel -like "*$($_)\*" }) } |
    Sort-Object FullName

function Get-RelPath([string]$full) {
    return $full.Substring($docsRoot.Length).TrimStart('\', '/')
}

# A heading line is level 2..4 (`## `, `### `, `#### `).
# Level 1 (`# `) is the document title and is counted too, separately.
function Test-Heading([string]$line) { return $line -match '^#{1,6}\s' }

$report = @()
$failures = @()

foreach ($f in $mdFiles) {
    $lines = [System.IO.File]::ReadAllLines($f.FullName, [Text.Encoding]::UTF8)
    $rel   = Get-RelPath $f.FullName

    $titleCount   = ($lines | Where-Object { $_ -match '^#\s' }).Count
    $headings     = @($lines | Where-Object { $_ -match '^#{2,4}\s' })
    $headingCount = $headings.Count

    # -- duplicate headings (exact text, level included) --------------------
    $dups = $headings | Group-Object | Where-Object { $_.Count -gt 1 }

    # -- code fences must pair up ------------------------------------------
    $fences = ($lines | Where-Object { $_ -match '^```' }).Count

    # -- level-1 count: REPORTED ONLY, never a failure ----------------------
    # `# ` is NOT a reliable document-title marker here: Markdown's `#` is
    # indistinguishable from a shell comment, and several docs legitimately
    # carry shell snippets in fenced blocks (a line like "# build the spike",
    # "# run the matrix", "# palette").
    # An earlier version treated "exactly 1 title" as a failure and produced
    # 7 false positives out of 16 files. A check that cries wolf gets ignored,
    # which is worse than not having it -- so this is informational only.
    #
    # Uniqueness, by contrast, DOES matter for the ##..#### levels, because
    # those are the anchor targets our cross-references depend on.
    if ($dups) {
        foreach ($d in $dups) {
            $failures += "$rel : duplicate heading x$($d.Count) : $($d.Name)"
        }
    }
    if ($fences % 2 -ne 0) {
        $failures += "$rel : code fences not paired (count = $fences)"
    }

    $report += [pscustomobject]@{
        file    = $rel
        title   = $titleCount
        headings = $headingCount
        fences  = $fences
        dups    = $dups.Count
    }
}

# ---------------------------------------------------------------------------
# Heading conservation against the baseline.
# ---------------------------------------------------------------------------
$current = @{}
foreach ($r in $report) { $current[$r.file] = $r.headings }

if ($UpdateBaseline) {
    $payload = [pscustomobject]@{
        note = 'Heading counts per docs file. Changing a count must be an explicit act (recorded in the commit that changes it).'
        generated_at = (Get-Date).ToString('s')
        files = $current
    }
    $payload | ConvertTo-Json -Depth 5 | Set-Content -Path $baselinePath -Encoding UTF8
    Write-Host "baseline updated: $baselinePath" -ForegroundColor Yellow
    Write-Host "  review the diff and commit it together with the change that caused it." -ForegroundColor Yellow
} elseif (Test-Path $baselinePath) {
    $base = (Get-Content $baselinePath -Raw -Encoding UTF8 | ConvertFrom-Json).files
    $baseMap = @{}
    foreach ($p in $base.PSObject.Properties) { $baseMap[$p.Name] = [int]$p.Value }

    foreach ($k in $baseMap.Keys) {
        if (-not $current.ContainsKey($k)) {
            $failures += "$k : file is in the baseline but no longer exists"
        } elseif ($current[$k] -ne $baseMap[$k]) {
            $delta = $current[$k] - $baseMap[$k]
            $sign  = if ($delta -gt 0) { "+$delta" } else { "$delta" }
            $failures += ("{0} : HEADING COUNT CHANGED {1} -> {2} ({3})" -f `
                $k, $baseMap[$k], $current[$k], $sign)
        }
    }
    foreach ($k in $current.Keys) {
        if (-not $baseMap.ContainsKey($k)) {
            $failures += "$k : new file, not in the baseline (run -UpdateBaseline to record it)"
        }
    }
} else {
    Write-Host "no baseline yet: $baselinePath" -ForegroundColor Yellow
    Write-Host "  run with -UpdateBaseline to record the current counts." -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
Write-Host ""
Write-Host ("{0,-52} {1,6} {2,8} {3,7} {4,5}" -f 'file', 'title', 'headings', 'fences', 'dups')
Write-Host ('-' * 82)
foreach ($r in $report) {
    Write-Host ("{0,-52} {1,6} {2,8} {3,7} {4,5}" -f $r.file, $r.title, $r.headings, $r.fences, $r.dups)
}

$totalHeadings = ($report | Measure-Object -Property headings -Sum).Sum
Write-Host ('-' * 82)
Write-Host ("{0,-52} {1,6} {2,8}" -f ("TOTAL (" + $report.Count + " files)"), "", $totalHeadings)

Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host "OK: all structural checks passed." -ForegroundColor Green
    exit 0
} else {
    Write-Host ("FAILED: " + $failures.Count + " problem(s)") -ForegroundColor Red
    $failures | ForEach-Object { Write-Host ("  " + $_) -ForegroundColor Red }
    Write-Host ""
    Write-Host "If a heading count change is INTENTIONAL, re-run with -UpdateBaseline" -ForegroundColor Yellow
    Write-Host "and commit the baseline together with that change." -ForegroundColor Yellow
    if ($Strict) { exit 1 } else { exit 0 }
}
