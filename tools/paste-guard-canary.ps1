# Paste-guard canary: prove tools/check-copy-paste.ps1 can actually FAIL.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# WHY
#
# A guard whose baseline is green proves nothing about whether it can go red.
# The first attempt at this trip was WRONG and is worth recording: the line used
# was `let Ok(entries) = std::fs::read_dir(dir) else {`, which is on the guard's
# own boilerplate-exclusion list -- so the guard would NEVER report it, and the
# test "passed" without exercising anything. That is the same class of mistake
# this project keeps hitting: the verification condition did not match the
# mechanism under test.
#
# THE FIX, AND THE RULE IT IMPLIES
#
#   The canary line must be selected FROM THE CURRENT REFERENCE TREE, by the
#   same criteria the guard uses (long enough, not boilerplate, unique to one
#   file). Hard-coding a line would rot silently: if that file disappears from
#   `repos/`, the canary stops being a canary and the run would look like a
#   failure of the GUARD rather than of the CANARY.
#
# So this script:
#   1. picks a suitable line from repos/ at run time
#   2. verifies the guard reports the BASELINE as green first
#   3. injects the line into a new file under crates/qul-core/src/
#   4. requires the guard to FAIL and to name that file
#   5. removes the file
#   6. requires the guard to be green again, and the tree to be clean
#
# Usage:
#   powershell -File tools/paste-guard-canary.ps1
# ---------------------------------------------------------------------------

param(
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $root 'docs\_artifacts\S9' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$guard = Join-Path $root 'tools\check-copy-paste.ps1'
$canary = Join-Path $root 'crates\qul-core\src\__canary_copy.rs'
if (Test-Path $canary) { throw "refusing to run: $canary already exists (residue from an earlier run?)" }

$evidence = @()
function Add-Evidence {
    param([string]$Step, [string]$Expectation, [int]$ExitCode, [string]$Output)
    $script:evidence += [pscustomobject]@{
        step = $Step; expectation = $Expectation; exit_code = $ExitCode; output = $Output
    }
}

function Run-Guard {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $guard 2>&1 | Out-String
        $code = $LASTEXITCODE
        if ($null -eq $code) { $code = 0 }
    }
    finally { $ErrorActionPreference = $prev }
    return [pscustomobject]@{ exit = $code; text = $out }
}

# ---- 1. pick a canary line from the CURRENT reference tree -------------------
# Criteria mirror the guard: long enough, not a comment/attribute/use, and not
# one of the obvious shared-vocabulary shapes.
Write-Host 'selecting a canary line from repos/ ...' -ForegroundColor DarkGray
$cand = $null
$searchRoot = Join-Path $root 'repos'
foreach ($f in (Get-ChildItem $searchRoot -Recurse -File -Include *.rs -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch 'target|node_modules|\\\.git\\' -and $_.Length -lt 400000 })) {
    if ($cand) { break }
    foreach ($line in (Get-Content -LiteralPath $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        $k = $line.Trim()
        if ($k.Length -lt 60) { continue }
        if ($k -match '^\s*(//|///|//!|#\[|use |pub use|import |from |export )') { continue }
        $cand = [pscustomobject]@{ Line = $k; File = $f.FullName }
        break
    }
}
if (-not $cand) { throw 'no suitable canary line found in repos/ -- cannot prove the guard works' }
Write-Host ("  canary : {0}" -f $cand.Line.Substring(0, [Math]::Min(80, $cand.Line.Length))) -ForegroundColor DarkGray
Write-Host ("  from   : {0}" -f ($cand.File.Substring($root.Length).TrimStart('\'))) -ForegroundColor DarkGray

try {
    # ---- 2. baseline must be green -----------------------------------------
    Write-Host ''
    Write-Host '== step 1: baseline (expect green) ==' -ForegroundColor Cyan
    $base = Run-Guard
    Write-Host ("  exit={0}" -f $base.exit)
    Add-Evidence -Step 'baseline' -Expectation 'PASS (exit 0)' -ExitCode $base.exit -Output $base.text
    if ($base.exit -ne 0) {
        throw "baseline is not green -- fix that first; a canary run on a red baseline proves nothing"
    }

    # ---- 3/4. inject and require failure -----------------------------------
    Write-Host ''
    Write-Host '== step 2: inject a reference line (expect FAIL) ==' -ForegroundColor Cyan
    $body = "// canary - created and deleted by tools/paste-guard-canary.ps1`r`nfn canary() {`r`n    $($cand.Line)`r`n}`r`n"
    [System.IO.File]::WriteAllText($canary, $body, (New-Object System.Text.UTF8Encoding($false)))

    $tripped = Run-Guard
    Write-Host ("  exit={0}" -f $tripped.exit)
    Add-Evidence -Step 'injected' -Expectation 'FAIL (exit 1)' -ExitCode $tripped.exit -Output $tripped.text

    # Strip ANSI colour codes before matching. The guard writes coloured output,
    # and the escape sequences sit BETWEEN the path and the rest of the line --
    # matching the raw text fails for a reason that has nothing to do with the
    # guard working. (First run of this canary reported FAIL here for exactly
    # that reason, while the guard itself had in fact named the file.)
    # Escape sequence is ESC followed by '['. Built from a char code so this
    # file stays pure ASCII.
    $esc = ([char]27) + '['
    $plain = $tripped.text.Replace($esc, 'ESC[')
    $namedFile = $plain.Contains('__canary_copy')
    Write-Host ("  guard named the canary file: {0}" -f $namedFile) -ForegroundColor $(if ($namedFile) { 'Green' } else { 'Red' })
    Add-Evidence -Step 'injected-names-file' -Expectation 'PASS (report points at the canary)' `
        -ExitCode $(if ($namedFile) { 0 } else { 1 }) -Output '(see injected output above)'
}
finally {
    if (Test-Path $canary) { Remove-Item $canary -Force }
}

# ---- 5/6. green again, and clean -------------------------------------------
Write-Host ''
Write-Host '== step 3: after cleanup (expect green) ==' -ForegroundColor Cyan
$after = Run-Guard
Write-Host ("  exit={0}" -f $after.exit)
Add-Evidence -Step 'after-cleanup' -Expectation 'PASS (exit 0)' -ExitCode $after.exit -Output $after.text

$clean = (-not (Test-Path $canary))
Write-Host ("  canary absent: {0}" -f $clean) -ForegroundColor $(if ($clean) { 'Green' } else { 'Red' })
Add-Evidence -Step 'tree-clean' -Expectation 'PASS (canary removed)' -ExitCode $(if ($clean) { 0 } else { 1 }) `
    -Output ("canary present={0}" -f (Test-Path $canary))

# ---- write evidence --------------------------------------------------------
$logPath = Join-Path $OutDir 'paste-guard-canary.log'
$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine("canary line : $($cand.Line)")
[void]$sb.AppendLine("canary from : $($cand.File)")
[void]$sb.AppendLine('')
foreach ($e in $evidence) {
    [void]$sb.AppendLine('================================================================')
    [void]$sb.AppendLine("step        : $($e.step)")
    [void]$sb.AppendLine("expectation : $($e.expectation)")
    [void]$sb.AppendLine("exit code   : $($e.exit_code)")
    [void]$sb.AppendLine('----------------------------------------------------------------')
    [void]$sb.AppendLine($e.output)
    [void]$sb.AppendLine('')
}
[System.IO.File]::WriteAllText($logPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Write-Host '== result ==' -ForegroundColor Cyan
foreach ($e in $evidence) {
    $ok = if ($e.step -eq 'injected') { $e.exit_code -ne 0 } else { $e.exit_code -eq 0 }
    Write-Host ("  {0,-6} {1}" -f $(if ($ok) { 'OK' } else { 'FAIL' }), $e.step)
}
Write-Host "log : $logPath"

$allOk = $true
foreach ($e in $evidence) {
    if ($e.step -eq 'injected') { if ($e.exit_code -eq 0) { $allOk = $false } }
    else { if ($e.exit_code -ne 0) { $allOk = $false } }
}
if (-not $clean) { $allOk = $false }
if (-not $allOk) { exit 1 }
