# =============================================================================
# s7-baseline.ps1 -- capture the OFFICIAL launcher's real java command line
#
# S7 step 5 requires comparing our assembled command line against the official
# one, parameter by parameter, and forbids "looks about right".
#
# The task book suggested capturing it from a RUNNING process with
# Win32_Process. That works, but it has a real limitation: you have to hit the
# timing window, and it only ever gives you the CURRENT launch. It happened to
# be unavailable here (no java process was running when I looked), and that is
# not a fluke -- the game is a foreground application the user closes.
#
# A BETTER SOURCE TURNED OUT TO EXIST: the official launcher writes every
# assembled argument to its own log, one per line, tagged
# `JavaLaunchConfiguration.cpp(281)] Java argument:<x>`. So instead of racing a
# process, we read history -- which is reproducible, reviewable, and lets us
# compare several launches.
#
# WHAT THIS SCRIPT PINS DOWN, AND WHY:
#
#   - WHICH session (by timestamp) the baseline came from, so a later re-run can
#     tell whether the baseline changed.
#   - The `<WORKDIR>` placeholder expansion. The log writes `<WORKDIR>` instead
#     of the real user path -- which is GOOD, because it means the baseline has
#     no absolute user path in it and can be committed without scrubbing.
#   - The raw argument list, one per line, in the order the launcher emitted it.
#     ORDER MATTERS for `-cp` and for the game's own argument parsing, so the
#     baseline is NOT sorted.
#
# NOTE (project rule): this file must stay ASCII-only.
#
# Usage:
#   pwsh -File spikes/s7-official-baseline/capture.ps1
# =============================================================================

$ErrorActionPreference = 'Stop'

$mc = Join-Path $env:APPDATA '.minecraft'
if (-not (Test-Path $mc)) {
    Write-Output "FAIL: official launcher data dir not found: $mc"
    exit 2
}

$outDir = $PSScriptRoot
$logs = Get-ChildItem $mc -Filter 'launcher_log*.txt' -ErrorAction SilentlyContinue
if ($logs.Count -eq 0) {
    Write-Output 'FAIL: no launcher_log*.txt found - has the official launcher ever run Java?'
    exit 2
}

# Collect every `Java argument:` line with its session timestamp.
$rows = @()
foreach ($f in $logs) {
    foreach ($line in (Get-Content $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        if ($line -notmatch 'Java argument:') { continue }
        if ($line -notmatch '\[Info: (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\.') { continue }
        $ts = $Matches[1]
        $at = $line.IndexOf('Java argument:')
        $val = $line.Substring($at + 'Java argument:'.Length)
        $rows += [pscustomobject]@{ session = $ts; log = $f.Name; value = $val }
    }
}

if ($rows.Count -eq 0) {
    Write-Output 'FAIL: the launcher logs contain no `Java argument:` lines.'
    Write-Output '      That means the official launcher never assembled a Java command'
    Write-Output '      line on this machine - launch the game once with it first.'
    exit 2
}

# The newest session is the baseline. (Same instant = same launch.)
$sessions = @($rows | Group-Object session | Sort-Object Name)
$latest = $sessions[-1]
$args0 = @($latest.Group | ForEach-Object { $_.value })

Write-Output '== S7 baseline: official launcher java command line =='
Write-Output ''
Write-Output ("  sessions found : {0}" -f $sessions.Count)
foreach ($s in $sessions) {
    $mark = if ($s.Name -eq $latest.Name) { '  <- baseline' } else { '' }
    Write-Output ("    {0}  {1,3} args  [{2}]{3}" -f $s.Name, $s.Count, $s.Group[0].log, $mark)
}
Write-Output ''
Write-Output ("  arguments      : {0}" -f $args0.Count)
$workdir = @($args0 | Where-Object { $_ -like '*<WORKDIR>*' }).Count
Write-Output ("  contain <WORKDIR> : {0}" -f $workdir)
Write-Output ''

# Write the raw baseline, one argument per line, unsorted.
$rawPath = Join-Path $outDir 'baseline-args.txt'
Set-Content -Path $rawPath -Value $args0 -Encoding ASCII
Write-Output ("  wrote {0}" -f $rawPath)

# A small structured summary: the first token is the java executable, then the
# flags, then whatever follows.
$first = $args0[0]
$summary = [ordered]@{
    session            = $latest.Name
    log_file           = $latest.Group[0].log
    arg_count          = $args0.Count
    java_executable    = $first
    sessions_available = $sessions.Count
}
$summaryPath = Join-Path $outDir 'baseline-summary.json'
[System.IO.File]::WriteAllText($summaryPath, ($summary | ConvertTo-Json -Depth 4), (New-Object System.Text.UTF8Encoding($false)))
Write-Output ("  wrote {0}" -f $summaryPath)
Write-Output ''

# Show what it looks like, so a human can sanity-check without opening files.
Write-Output '  --- first 12 arguments ---'
$args0 | Select-Object -First 12 | ForEach-Object { Write-Output ("    {0}" -f $_) }
Write-Output '  --- last 8 arguments ---'
$args0 | Select-Object -Last 8 | ForEach-Object { Write-Output ("    {0}" -f $_) }
Write-Output ''
Write-Output 'OK: baseline captured. It contains <WORKDIR>, not a real user path,'
Write-Output '    so it can be committed as-is.'
exit 0
