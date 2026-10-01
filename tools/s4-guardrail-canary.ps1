# S4 guardrail canary: trip each guard on purpose, keep the evidence, then clean up.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing). Reasoning that would
# otherwise live in Chinese comments is recorded in the M0 spike report instead.
#
# WHY THIS EXISTS
#
# The spike task book (S4) sets the pass bar as:
#
#     "the three guardrails can all be tripped ON PURPOSE"
#
# It does NOT say "the guardrails pass". A check that is always green is
# indistinguishable from no check at all -- and it is WORSE, because it gives
# the impression that something is protected.
#
# So this script does what the task book asks, in the only honest way:
#
#   1. snapshot the artifacts it will touch (by hash, or by explicit absence)
#   2. create something that MUST be rejected
#   3. run the REAL check and CAPTURE ITS FAILURE OUTPUT as the evidence
#   4. remove the canary
#   5. re-run the same check and require it to be green again
#   6. verify every touched artifact equals the snapshot
#
# Steps 5 and 6 are the ones usually skipped. Without 5 you cannot tell "the
# guard tripped" from "I broke the build". Without 6 you leave canary files
# behind, and the next person finds a red guard with no idea why.
#
# THE THREE GUARDS
#
#   A  qul-core must not depend on IO / UI crates
#      canary: a line appended to crates/qul-core/Cargo.toml, because the
#      architecture test reads that file -- a real edit is the only faithful
#      canary. Removed afterwards.
#
#   B  components must not call fetch() directly
#   C  components must not import the IPC bridge directly
#      canary: web/src/__canary__.ts, i.e. under web/src but OUTSIDE
#      web/src/api/, which is the only place allowed to touch the outside
#      world. eslint runs on the real config, so this proves the rule fires on
#      a real file in the real tree.
#
# Usage:
#   powershell -File tools/s4-guardrail-canary.ps1
# ---------------------------------------------------------------------------

param(
    [string]$OutDir = ""
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $root 'docs\_artifacts\S4'
}
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$coreToml = Join-Path $root 'crates\qul-core\Cargo.toml'
$canaryTs = Join-Path $root 'web\src\__canary__.ts'

# -- toolchain discovery ------------------------------------------------------
# cargo is NOT on PATH in this environment (it lives under the user profile).
# Resolving it explicitly is better than requiring the caller to fix their PATH:
# a canary that fails because of the environment teaches nothing.
function Resolve-Cargo {
    $cmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @()
    if ($env:USERPROFILE) { $candidates += (Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe') }
    if ($env:CARGO_HOME) { $candidates += (Join-Path $env:CARGO_HOME 'bin\cargo.exe') }
    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    throw "cargo not found (looked on PATH and in %USERPROFILE%\.cargo\bin)"
}

$cargoExe = Resolve-Cargo
Write-Host "cargo: $cargoExe" -ForegroundColor DarkGray

$evidence = @()
function Add-Evidence {
    param([string]$Guard, [string]$Expectation, [string]$Command, [int]$ExitCode, [string]$Output)
    $script:evidence += [pscustomobject]@{
        guard       = $Guard
        expectation = $Expectation
        command     = $Command
        exit_code   = $ExitCode
        output      = $Output
    }
}

# Runs a command, capturing combined output and exit code WITHOUT letting a
# non-zero exit abort the script -- a non-zero exit is the expected result here.
function Run-Capture {
    param([string]$WorkDir, [string[]]$Argv)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        Push-Location $WorkDir
        $out = & $Argv[0] @($Argv[1..($Argv.Count - 1)]) 2>&1 | Out-String
        $code = $LASTEXITCODE
        Pop-Location
        if ($null -eq $code) { $code = 0 }
    }
    finally { $ErrorActionPreference = $prev }
    return [pscustomobject]@{ exit = $code; text = $out }
}

Write-Host "root : $root" -ForegroundColor Cyan
Write-Host "out  : $OutDir" -ForegroundColor Cyan

# -- snapshot -----------------------------------------------------------------
$snapToml = (Get-FileHash $coreToml -Algorithm SHA256).Hash
$canaryExistedBefore = Test-Path $canaryTs
if ($canaryExistedBefore) { throw "refusing to run: $canaryTs already exists (residue from an earlier run?)" }
Write-Host ("snapshot: crates/qul-core/Cargo.toml sha256={0}" -f $snapToml.Substring(0, 16)) -ForegroundColor DarkGray

try {
    # ============ guard A: core must not gain an IO dependency ==============
    Write-Host ""
    Write-Host "== guard A: qul-core Cargo.toml gains 'reqwest' ==" -ForegroundColor Cyan
    Add-Content -Path $coreToml -Value "reqwest = `"0.12`"" -Encoding UTF8

    $a = Run-Capture -WorkDir $root -Argv @($cargoExe, 'test', '-p', 'qul-core', '--test', 'architecture')
    Write-Host ("  exit={0}" -f $a.exit) -ForegroundColor $(if ($a.exit -ne 0) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'A core-no-io-deps' -Expectation 'FAIL (guard must trip)' `
        -Command 'cargo test -p qul-core --test architecture' -ExitCode $a.exit -Output $a.text
    if ($a.exit -eq 0) { Write-Host "  !! guard A did NOT trip -- that is a finding, not a pass" -ForegroundColor Red }

    # restore by removing exactly the appended line, then verify by hash
    $restoreToml = (Get-Content $coreToml -Raw -Encoding UTF8) -replace "(?m)^reqwest = `"0\.12`"\r?\n", ''
    [System.IO.File]::WriteAllText($coreToml, $restoreToml, (New-Object System.Text.UTF8Encoding($false)))

    $afterA = (Get-FileHash $coreToml -Algorithm SHA256).Hash
    $aRestored = ($afterA -eq $snapToml)
    Write-Host ("  restored byte-identical: {0}" -f $aRestored) -ForegroundColor $(if ($aRestored) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'A restore' -Expectation 'PASS (file byte-identical to snapshot)' `
        -Command 'sha256(crates/qul-core/Cargo.toml) == snapshot' -ExitCode $(if ($aRestored) { 0 } else { 1 }) `
        -Output ("after={0}`nsnapshot={1}" -f $afterA, $snapToml)

    $a2 = Run-Capture -WorkDir $root -Argv @($cargoExe, 'test', '-p', 'qul-core', '--test', 'architecture')
    Write-Host ("  green again: exit={0}" -f $a2.exit) -ForegroundColor $(if ($a2.exit -eq 0) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'A after-cleanup' -Expectation 'PASS (guard green again)' `
        -Command 'cargo test -p qul-core --test architecture' -ExitCode $a2.exit -Output $a2.text

    # ============ guard B: a component calls fetch() directly ===============
    Write-Host ""
    Write-Host "== guard B: web/src/__canary__.ts calls fetch() ==" -ForegroundColor Cyan
    $canaryB = @'
// S4 CANARY - intentionally violates guardrail B. Deleted by the harness.
//
// This file lives OUTSIDE web/src/api/, which is the only place allowed to
// touch the network or the IPC bridge.
export async function canary(): Promise<unknown> {
  const r = await fetch("https://example.invalid/");
  return r;
}
'@
    [System.IO.File]::WriteAllText($canaryTs, $canaryB, (New-Object System.Text.UTF8Encoding($false)))

    $b = Run-Capture -WorkDir $root -Argv @('pnpm', 'run', 'lint')
    Write-Host ("  exit={0}" -f $b.exit) -ForegroundColor $(if ($b.exit -ne 0) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'B component-no-fetch' -Expectation 'FAIL (rule fires on a real file)' `
        -Command 'pnpm run lint' -ExitCode $b.exit -Output $b.text
    if ($b.exit -eq 0) { Write-Host "  !! guard B did NOT trip" -ForegroundColor Red }

    # ============ guard C: a component imports the IPC bridge ===============
    Write-Host ""
    Write-Host "== guard C: web/src/__canary__.ts imports the IPC bridge ==" -ForegroundColor Cyan
    $canaryC = @'
// S4 CANARY - intentionally violates guardrail C. Deleted by the harness.
import { invoke } from "@tauri-apps/api/core";

export async function canaryIpc(): Promise<unknown> {
  return invoke("capability_overview");
}
'@
    [System.IO.File]::WriteAllText($canaryTs, $canaryC, (New-Object System.Text.UTF8Encoding($false)))

    $c = Run-Capture -WorkDir $root -Argv @('pnpm', 'run', 'lint')
    Write-Host ("  exit={0}" -f $c.exit) -ForegroundColor $(if ($c.exit -ne 0) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'C component-no-direct-ipc' -Expectation 'FAIL (rule fires on a real file)' `
        -Command 'pnpm run lint' -ExitCode $c.exit -Output $c.text
    if ($c.exit -eq 0) { Write-Host "  !! guard C did NOT trip" -ForegroundColor Red }

    # clean up and re-run
    Remove-Item $canaryTs -Force
    $d = Run-Capture -WorkDir $root -Argv @('pnpm', 'run', 'lint')
    Write-Host ("  green again: exit={0}" -f $d.exit) -ForegroundColor $(if ($d.exit -eq 0) { 'Green' } else { 'Red' })
    Add-Evidence -Guard 'B/C after-cleanup' -Expectation 'PASS (lint green again)' `
        -Command 'pnpm run lint' -ExitCode $d.exit -Output $d.text
}
finally {
    # unconditional cleanup so an abort mid-way cannot leave a canary behind
    if (Test-Path $canaryTs) { Remove-Item $canaryTs -Force }
}

# -- final tree verification --------------------------------------------------
$finalToml = (Get-FileHash $coreToml -Algorithm SHA256).Hash
$treeClean = ($finalToml -eq $snapToml) -and (-not (Test-Path $canaryTs))
Write-Host ""
Write-Host ("tree clean (Cargo.toml identical AND canary absent): {0}" -f $treeClean) -ForegroundColor $(if ($treeClean) { 'Green' } else { 'Red' })
Add-Evidence -Guard 'tree-clean' -Expectation 'PASS (no residue)' `
    -Command 'sha256 Cargo.toml AND Test-Path canary' -ExitCode $(if ($treeClean) { 0 } else { 1 }) `
    -Output ("Cargo.toml after={0}`nsnapshot={1}`ncanary present={2}" -f $finalToml, $snapToml, (Test-Path $canaryTs))

# -- write evidence -----------------------------------------------------------
$summary = [pscustomobject]@{
    generated_at = (Get-Date).ToString('s')
    root         = $root
    guards       = @($evidence | ForEach-Object {
            [pscustomobject]@{
                guard       = $_.guard
                expectation = $_.expectation
                command     = $_.command
                exit_code   = $_.exit_code
            }
        })
}
$jsonPath = Join-Path $OutDir 's4-guardrail-canary.json'
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8

# Full transcripts are kept separately: they ARE the evidence. A summary without
# them cannot be re-read by someone who was not present.
$logPath = Join-Path $OutDir 's4-guardrail-canary.log'
$sb = New-Object System.Text.StringBuilder
foreach ($e in $evidence) {
    [void]$sb.AppendLine("================================================================")
    [void]$sb.AppendLine("guard       : $($e.guard)")
    [void]$sb.AppendLine("expectation : $($e.expectation)")
    [void]$sb.AppendLine("command     : $($e.command)")
    [void]$sb.AppendLine("exit code   : $($e.exit_code)")
    [void]$sb.AppendLine("----------------------------------------------------------------")
    [void]$sb.AppendLine($e.output)
    [void]$sb.AppendLine("")
}
[System.IO.File]::WriteAllText($logPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ""
Write-Host "== result ==" -ForegroundColor Cyan
foreach ($e in $evidence) {
    $isCheck = ($e.guard -like '*after-cleanup*') -or ($e.guard -like '*restore*') -or ($e.guard -eq 'tree-clean')
    if ($isCheck) {
        $mark = if ($e.exit_code -eq 0) { 'OK' } else { 'FAIL' }
    }
    else {
        $mark = if ($e.exit_code -ne 0) { 'TRIPPED' } else { 'NOT-TRIPPED' }
    }
    Write-Host ("  {0,-12} {1}" -f $mark, $e.guard)
}
Write-Host "summary : $jsonPath"
Write-Host "log     : $logPath"

if (-not $treeClean) { exit 1 }
