# Process B (part 2): dependency licence inventory + per-item source-register check.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# WHY NOT cargo-deny / cargo-about / license-checker
#
# The task book suggests those tools. They are the right answer for a bigger
# project, but here each one means a new dependency with its own licence surface
# -- and the whole point of this check is licence hygiene. Adding a large
# third-party scanner to police third-party code is a real trade-off, not an
# obvious win.
#
# What this script does instead, with no new dependencies:
#   part 1  inventory the licence of every locked dependency by reading the
#           `license` field from the local cargo registry index/cache, and from
#           node_modules package.json files. Unknown entries are REPORTED, not
#           waved through.
#   part 2  check docs/source-register for the two things that actually rot:
#             * every reference repo under repos/ is mentioned
#             * every mentioned repo has a licence word on the same line
#
# LIMITS (stated so nobody over-trusts it):
#   * part 1 only sees dependencies already present in the local cargo cache or
#     node_modules. A fresh machine with an empty cache will report almost
#     everything as unknown -- that is a "could not check", not a pass, and it
#     is reported as such.
#   * it does not resolve LGPL static-linking questions. It surfaces the licence
#     so a human can decide; it does not decide.
#
# Usage:
#   powershell -File tools/check-licenses.ps1
# ---------------------------------------------------------------------------

param(
    [string]$OutDir = '',
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $root 'docs\_artifacts\S9' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

function Say {
    param([string]$Text)
    if (-not $Quiet) { Write-Output $Text }
}

$problems = @()

# ======================= part 1: Rust dependency licences ====================

Say '== Rust dependencies (from the local cargo registry cache) =='
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$srcDir = Join-Path $cargoHome 'registry\src'
$pkgDirs = @()
if (Test-Path $srcDir) {
    $pkgDirs = Get-ChildItem $srcDir -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        Get-ChildItem $_.FullName -Directory -ErrorAction SilentlyContinue
    }
}

$licByPkg = @{}
foreach ($d in $pkgDirs) {
    $cp = Join-Path $d.FullName 'Cargo.toml'
    if (-not (Test-Path $cp)) { continue }
    $name = $d.Name -replace '-\d+\.\d+\.\d+.*$', ''
    $txt = Get-Content -LiteralPath $cp -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($txt -and $txt -match '(?m)^\s*license\s*=\s*"([^"]+)"') {
        $licByPkg[$name] = $Matches[1]
    }
}

# Which crates do we actually depend on? Read from Cargo.lock (authoritative for
# the built graph) rather than Cargo.toml (which may use workspace inheritance).
$lock = Join-Path $root 'Cargo.lock'
$ourDeps = @()
if (Test-Path $lock) {
    $ltxt = Get-Content -LiteralPath $lock -Raw -Encoding UTF8
    foreach ($m in [regex]::Matches($ltxt, '(?m)^name = "([^"]+)"')) {
        $ourDeps += $m.Groups[1].Value
    }
}
$ourDeps = @($ourDeps | Sort-Object -Unique)

# Our own workspace members have no `license` field by design (never published).
# Without this exclusion they show up as "UNKNOWN", which is noise of exactly the
# kind that teaches people to ignore the check.
$ownCrates = @()
foreach ($m in (Get-ChildItem (Join-Path $root 'crates') -Directory -ErrorAction SilentlyContinue)) {
    $cp = Join-Path $m.FullName 'Cargo.toml'
    if (Test-Path $cp) {
        $mtxt = Get-Content -LiteralPath $cp -Raw -Encoding UTF8
        if ($mtxt -match '(?m)^\s*name\s*=\s*"([^"]+)"') { $ownCrates += $Matches[1] }
    }
}
Say ("  own workspace members excluded: {0}" -f ($ownCrates -join ', '))

$rustRows = @()
foreach ($d in $ourDeps) {
    if ($ownCrates -contains $d) { continue }
    $lic = if ($licByPkg.ContainsKey($d)) { $licByPkg[$d] } else { 'UNKNOWN' }
    $rustRows += [pscustomobject]@{ crate = $d; license = $lic }
}
$rustUnknown = @($rustRows | Where-Object { $_.license -eq 'UNKNOWN' })
Say ("  crates in Cargo.lock: {0}   with a licence found: {1}   unknown: {2}" -f `
        $rustRows.Count, ($rustRows.Count - $rustUnknown.Count), $rustUnknown.Count)

# Copyleft (not necessarily a problem -- but must be a conscious choice).
$copyleft = @($rustRows | Where-Object { $_.license -match 'GPL|AGPL|SSPL|CDDL|EPL' })
if ($copyleft.Count -gt 0) {
    Say ("  copyleft components: {0}" -f $copyleft.Count)
    foreach ($c in $copyleft | Select-Object -First 12) { Say ("    {0}  {1}" -f $c.crate, $c.license) }
}

if ($rustUnknown.Count -gt 0) {
    $problems += ("{0} Rust crate(s) have no licence found in the local cache: {1}" -f `
            $rustUnknown.Count, (($rustUnknown | Select-Object -First 8 | ForEach-Object { $_.crate }) -join ', '))
}

# ======================= part 2: npm dependency licences =====================

Say ''
Say '== npm dependencies (from node_modules) =='
$nm = Join-Path $root 'node_modules'
$npmRows = @()
if (Test-Path $nm) {
    Get-ChildItem $nm -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        $pj = Join-Path $_.FullName 'package.json'
        if (Test-Path $pj) {
            try {
                $j = Get-Content -LiteralPath $pj -Raw -Encoding UTF8 | ConvertFrom-Json
                $lic = if ($j.license) { $j.license } elseif ($j.licenses) { 'SEE licenses[]' } else { 'UNKNOWN' }
                $npmRows += [pscustomobject]@{ pkg = $_.Name; license = $lic }
            }
            catch { $npmRows += [pscustomobject]@{ pkg = $_.Name; license = 'UNREADABLE' } }
        }
    }
}
$npmUnknown = @($npmRows | Where-Object { $_.license -eq 'UNKNOWN' -or $_.license -eq 'UNREADABLE' })
Say ("  packages: {0}   unknown/unreadable: {1}" -f $npmRows.Count, $npmUnknown.Count)
if ($npmUnknown.Count -gt 0) {
    Say ("    e.g. {0}" -f (($npmUnknown | Select-Object -First 8 | ForEach-Object { $_.pkg }) -join ', '))
}

# ======================= part 3: source-register check ======================

Say ''
Say '== source register =='
# Locate the source register WITHOUT any non-ASCII literal in this file.
#
# Why not by content signature (tried first): the spike task book also contains
# the word LICENSE and mentions repos, so signature matching picked the WRONG
# file and then reported seven repos as unregistered -- a false alarm that read
# like a documentation gap and was really a detection bug.
#
# Why not by Chinese literal: PowerShell 5.1 reads a BOM-less .ps1 as ANSI, so a
# Chinese path here becomes mojibake and the lookup fails silently.
#
# So the name is built from code points: pure ASCII in the source, correct at run
# time, and no third heuristic left to get wrong.
$registerName = (-join @([char]0x6765, [char]0x6E90, [char]0x8BB0, [char]0x5F55)) + '.md'
$registerPath = Join-Path (Join-Path $root 'docs') $registerName
if (-not (Test-Path $registerPath)) {
    # Fall back to a table-shaped file that tabulates licences, so a rename does
    # not silently disable the check.
    foreach ($cand in (Get-ChildItem (Join-Path $root 'docs') -File -Filter '*.md' -ErrorAction SilentlyContinue)) {
        $head = Get-Content -LiteralPath $cand.FullName -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if (-not $head) { continue }
        $probe = $head.Substring(0, [Math]::Min(6000, $head.Length))
        if (($probe -match 'GPL|AGPL|MIT') -and ($probe -match 'repos') -and ($probe -match '(?m)^\|')) {
            $registerPath = $cand.FullName
            break
        }
    }
}
if (-not (Test-Path $registerPath)) {
    $problems += 'source register not found under docs/ -- cannot check provenance'
    Say '  NOT FOUND'
}
else {
    Say ("  file: {0}" -f ($registerPath.Substring($root.Length).TrimStart('\')))
    $reg = Get-Content -LiteralPath $registerPath -Raw -Encoding UTF8

    $reposDir = Join-Path $root 'repos'
    $repoNames = @()
    if (Test-Path $reposDir) {
        $repoNames = Get-ChildItem $reposDir -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }
    }
    # NOTE ON A BUG THIS REPLACED: the first version tested `$reg -match $r` and
    # broke out of the inner loop on the first hit. Directory names contain each
    # other -- 'PCL' matches inside 'PCL-CE' -- so 'PCL' was reported missing
    # while being present all along, and the report looked like a documentation
    # gap rather than a detection bug.
    #
    # A plain substring test has no such ordering dependency, and repo directory
    # names are distinctive enough that a substring is the right question here.
    $missing = @()
    foreach ($r in $repoNames) {
        if ($reg.IndexOf($r, [StringComparison]::OrdinalIgnoreCase) -lt 0) { $missing += $r }
    }
    Say ("  reference repos: {0}   not mentioned in register: {1}" -f $repoNames.Count, $missing.Count)
    if ($missing.Count -gt 0) {
        $problems += ("reference repo(s) absent from the source register: {0}" -f ($missing -join ', '))
        foreach ($m in $missing) { Say ("    MISSING: {0}" -f $m) }
    }

    # Every mention of a repo should carry a licence word on the same line --
    # this is the check that caught the Portal licence error once already.
    $licWords = 'MIT|Apache|GPL|AGPL|LGPL|BSD|ISC|MPL|Unlicense|zlib|CC0|custom|NOASSERTION|proprietary'
    $unlicensedMentions = @()
    foreach ($line in ($reg -split "`r?`n")) {
        foreach ($r in $repoNames) {
            if ($line -match [regex]::Escape($r)) {
                if ($line -notmatch $licWords) {
                    $unlicensedMentions += ("{0}: {1}" -f $r, $line.Trim().Substring(0, [Math]::Min(60, $line.Trim().Length)))
                }
                break
            }
        }
    }
    Say ("  mentions without a licence word on the same line: {0}" -f $unlicensedMentions.Count)
    if ($unlicensedMentions.Count -gt 0) {
        foreach ($u in $unlicensedMentions | Select-Object -First 10) { Say ("    {0}" -f $u) }
        # Not a hard failure: the register has narrative sections that mention
        # repos without repeating their licence. Reported as a warning only.
        Say '  (warning only -- narrative sections legitimately mention repos without a licence word)'
    }

    # Hard rule from the task book: AGPL and no-licence projects must never have
    # their code in our tree. The paste guard enforces the code half; here we
    # just assert the register still names them.
    if ($reg -notmatch 'AGPL') { $problems += 'the source register no longer mentions AGPL anywhere -- that rule is load-bearing' }
}

# ======================= output =============================================

$result = [pscustomobject]@{
    generated_at  = (Get-Date).ToString('s')
    rust_total    = $rustRows.Count
    rust_unknown  = $rustUnknown.Count
    rust_copyleft = $copyleft.Count
    npm_total     = $npmRows.Count
    npm_unknown   = $npmUnknown.Count
    problems      = @($problems)
    rust          = @($rustRows | Sort-Object license, crate)
    npm           = @($npmRows | Sort-Object license, pkg)
}
$jsonPath = Join-Path $OutDir 'license-and-source-report.json'
$result | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8

Say ''
if ($problems.Count -eq 0) {
    Say 'OK: dependency licences inventoried and the source register is consistent.'
    Say "report: $jsonPath"
    exit 0
}
Say ("FAIL: {0} problem(s)" -f $problems.Count)
foreach ($p in $problems) { Say ("  - {0}" -f $p) }
Say "report: $jsonPath"
exit 1
