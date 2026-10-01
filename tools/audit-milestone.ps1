# Process C: milestone audit. Mechanical checks only; judgement stays with a human.
#
# ---------------------------------------------------------------------------
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY.
#
# PowerShell 5.1 decodes a .ps1 WITHOUT a UTF-8 BOM as ANSI. The Chinese search
# terms this audit needs therefore CANNOT be written as literals -- they turn into
# mojibake, break the here-strings, and produce parse errors that point at the
# wrong line (this file was first written with literals and failed exactly that
# way, 67 non-ASCII bytes). They are built from code points instead. That keeps
# the file pure ASCII while still matching the Chinese text in docs/.
#
# WHY AN AUDIT SCRIPT AT ALL
#
# The plan made "run a full audit at every milestone exit" a rule, but for a long
# time nothing carried it -- a promise with no landing place. When it was finally
# run by hand it caught a real factual error (Portal's licence written as
# GPL-3.0 when it is AGPL-3.0), and a later round caught four more of the same
# kind. Anchors cannot rot-proof prose; only periodic re-reading can.
#
# WHAT THIS CAN AND CANNOT DO -- read before trusting it
#
#   CAN:  the DETERMINISTIC half -- dangling references, dangling promises,
#         licence words that disagree with the register, count drift. These are
#         exactly the failures that were found by hand.
#
#   CANNOT: decide whether a reference "means the same thing" as its target, or
#         whether a number is the RIGHT number. The task book's checks 1-2 ask
#         for semantic judgement. This script reports candidates and refuses to
#         pretend it can judge them.
#
# Output is a CANDIDATE LIST, not a verdict. Presenting candidates as findings
# would be the over-claiming this project keeps trying to avoid.
#
# Usage:
#   powershell -File tools/audit-milestone.ps1
#   powershell -File tools/audit-milestone.ps1 -OutDir docs/_artifacts/audit-M0
# ---------------------------------------------------------------------------

param(
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$docs = Join-Path $root 'docs'
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $docs '_artifacts\audit' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

# ---- Chinese search terms, built from code points --------------------------
# 0x5F85 = dai (await), 0x5B9E = shi (real), 0x6D4B = ce (measure),
# 0x8865 = bu (fill),   0x5B9A = ding (settle), 0x4E2A = ge (counter),
# 0x53C2 = can (refer), 0x8003 = kao, 0x4ED3 = cang (warehouse/repo)
$DAI = [string][char]0x5F85
$SHI = [string][char]0x5B9E
$CE = [string][char]0x6D4B
$BU = [string][char]0x8865
$DING = [string][char]0x5B9A
$GE = [string][char]0x4E2A
$CANKAO = (-join @([char]0x53C2, [char]0x8003))
$CANGKU = (-join @([char]0x4ED3, [char]0x5E93))
# register file name: 4 Chinese chars + .md (see the codepoints below)
$REGISTER_NAME = (-join @([char]0x6765, [char]0x6E90, [char]0x8BB0, [char]0x5F55)) + '.md'

$mdFiles = @(Get-ChildItem $docs -Recurse -File -Filter '*.md' -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\_artifacts\\' })
$rootMd = @('README.md', 'SESSION.md') | ForEach-Object { Join-Path $root $_ } |
    Where-Object { Test-Path $_ }
$allMd = @($mdFiles.FullName) + @($rootMd)

$findings = @()
function Add-Finding {
    param([string]$Kind, [string]$Severity, [string]$Where, [string]$Detail)
    $script:findings += [pscustomobject]@{ kind = $Kind; severity = $Severity; where = $Where; detail = $Detail }
}

# ===================== check 1: section references resolve ===================
#
# Every "section N.M" reference must have a matching heading somewhere in docs/.
# Per-file resolution is deliberately NOT attempted: docs cross-reference each
# other constantly, and a per-file rule would flood the report with false
# positives -- which would bury the real ones.
Write-Output '== check 1: do referenced section numbers exist? =='
$headings = @{}
foreach ($f in $allMd) {
    foreach ($line in (Get-Content -LiteralPath $f -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        if ($line -match '^#{1,6}\s+([0-9]+(?:\.[0-9]+)*|[A-Z][0-9]+)\.?\s') {
            $headings[$Matches[1]] = $true
        }
    }
}
Write-Output ("  distinct section numbers defined in docs: {0}" -f $headings.Count)

$refPattern = [regex]('[' + [char]0x00A7 + ']\s*([0-9]+(?:\.[0-9]+)*)')
$dangling = @{}
foreach ($f in $allMd) {
    $text = Get-Content -LiteralPath $f -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $text) { continue }
    foreach ($m in $refPattern.Matches($text)) {
        $ref = $m.Groups[1].Value
        if (-not $headings.ContainsKey($ref)) {
            if (-not $dangling.ContainsKey($ref)) { $dangling[$ref] = @() }
            $dangling[$ref] += ($f.Substring($root.Length).TrimStart('\'))
        }
    }
}
Write-Output ("  referenced but not found as a heading: {0}" -f $dangling.Count)
foreach ($k in ($dangling.Keys | Sort-Object)) {
    $where = (@($dangling[$k]) | Select-Object -Unique) -join ', '
    Add-Finding -Kind 'dangling-section-ref' -Severity 'P1' -Where $where -Detail ("section {0} has no matching heading" -f $k)
    Write-Output ("    section {0}   <- {1}" -f $k, $where)
}

# ===================== check 2: dangling promises ============================
Write-Output ''
Write-Output '== check 2: promises that may have no landing place =='
$promisePatterns = @(
    @{ p = $DAI + 'M0' + $SHI + $CE; d = 'defers to an M0 measurement' },
    @{ p = $DAI + $SHI + $CE; d = 'defers to a measurement' },
    @{ p = $DAI + $BU; d = 'says it will be filled in later' },
    @{ p = $DAI + $DING; d = 'left undecided' },
    @{ p = 'TODO'; d = 'explicit TODO' }
)
$promises = @()
foreach ($f in $allMd) {
    $lines = Get-Content -LiteralPath $f -Encoding UTF8 -ErrorAction SilentlyContinue
    $no = 0
    foreach ($line in $lines) {
        $no++
        foreach ($pp in $promisePatterns) {
            if ($line -match [regex]::Escape($pp.p)) {
                $promises += [pscustomobject]@{
                    file = $f.Substring($root.Length).TrimStart('\')
                    line = $no
                    kind = $pp.d
                    text = $line.Trim().Substring(0, [Math]::Min(90, $line.Trim().Length))
                }
                break
            }
        }
    }
}
Write-Output ("  lines containing a deferral phrase: {0}" -f $promises.Count)
Write-Output '  (candidates only: deferring a decision to a later milestone can be entirely legitimate)'
foreach ($p in $promises | Select-Object -First 25) {
    Write-Output ("    {0}:{1}  [{2}]" -f $p.file, $p.line, $p.kind)
}
if ($promises.Count -gt 25) { Write-Output ("    ... and {0} more (see json)" -f ($promises.Count - 25)) }

# KNOWN LIMITATION, recorded rather than hidden.
#
# This check cannot tell a HISTORICAL mention from a LIVE cross-reference. The
# plan's changelog says "v3.3 added section 13.x" -- true when written, and the
# section was later renumbered to 12.x. Flagging that as dangling would invite
# somebody to "fix" history, which is worse than the original state.
#
# So references inside changelog rows are reported but must be read with that in
# mind. Do not renumber history to make an audit green.

# ===================== check 3: licence words vs the register ================
#
# The check that caught the Portal error. Any other doc that states a licence for
# a reference repo must agree with the register.
Write-Output ''
Write-Output '== check 3: do other docs agree with the source register on licences? =='
$registerPath = Join-Path $docs $REGISTER_NAME
$repoNames = @()
$reposDir = Join-Path $root 'repos'
if (Test-Path $reposDir) { $repoNames = @(Get-ChildItem $reposDir -Directory | ForEach-Object { $_.Name }) }

$mismatches = @()
if ((Test-Path $registerPath) -and $repoNames.Count -gt 0) {
    $regText = Get-Content -LiteralPath $registerPath -Raw -Encoding UTF8
    $regLicence = @{}
    foreach ($line in ($regText -split "`r?`n")) {
        foreach ($r in $repoNames) {
            if ($line.IndexOf($r, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                if ($line -match '(AGPL|LGPL|GPL|Apache|MIT|BSD|MPL|Unlicense|custom|NOASSERTION)') {
                    if (-not $regLicence.ContainsKey($r)) { $regLicence[$r] = @() }
                    $regLicence[$r] += $Matches[1]
                }
            }
        }
    }
    $licenceKind = '\b(?:AGPL|LGPL|GPL|Apache|MIT|BSD|MPL|Unlicense)\b'
    foreach ($f in $allMd) {
        if ($f -eq $registerPath) { continue }
        $no = 0
        foreach ($line in (Get-Content -LiteralPath $f -Encoding UTF8 -ErrorAction SilentlyContinue)) {
            $no++
            foreach ($r in $repoNames) {
                if ($line.IndexOf($r, [StringComparison]::OrdinalIgnoreCase) -lt 0) { continue }
                # A line that names MORE THAN ONE licence cannot be attributed
                # to a specific repo. The first version took the first licence
                # word on the line and produced false positives on perfectly
                # correct text, e.g. "LeviLauncher = GPL-3.0 . Portal = AGPL-3.0"
                # (reported Portal as GPL) and an ADR row that merely mentions
                # the words for licence and AGPL in the same sentence.
                #
                # This detector is a CANDIDATE FINDER, not a linter: it is
                # allowed to miss things, but it must not cry wolf, because a
                # report people learn to skim is worse than no report.
                #
                # Three word-boundary traps were found by reading the first
                # report's hits by hand -- all three are recorded because each is
                # the same mistake in a different disguise:
                #   1. 'AGPL' matched inside a sentence about a DIFFERENT repo,
                #      because bold markers paired across the two names and the
                #      two ended up in one bold run;
                #   2. 'MIT' matched inside the word "architect";
                #   3. a line naming two licences was attributed to one repo.
                # So: word-bounded licence, an ODD number of bold markers (the
                # name and the licence are in the same bold run), and the licence
                # must FOLLOW the repo name on the line.
                if (($line.ToCharArray() | Where-Object { $_ -eq '*' }).Count % 2 -ne 1) { continue }
                $allKinds = [regex]::Matches($line, $licenceKind)
                if ($allKinds.Count -ne 1) { continue }
                $m = $allKinds[0]
                $repoPos = $line.IndexOf($r, [StringComparison]::OrdinalIgnoreCase)
                if ($repoPos -lt 0 -or $m.Index -lt $repoPos) { continue }
                if (-not $regLicence.ContainsKey($r)) { continue }
                $statedKinds = @($regLicence[$r] | Select-Object -Unique)
                if ($statedKinds -notcontains $m.Value) {
                    $mismatches += [pscustomobject]@{
                        file = $f.Substring($root.Length).TrimStart('\'); line = $no
                        repo = $r; stated = $m.Value; register = ($statedKinds -join '/')
                        text = $line.Trim().Substring(0, [Math]::Min(90, $line.Trim().Length))
                    }
                }
            }
        }
    }
}
Write-Output ("  licence mismatches vs register: {0}" -f $mismatches.Count)
foreach ($m in $mismatches) {
    Add-Finding -Kind 'licence-mismatch' -Severity 'P0' -Where ("{0}:{1}" -f $m.file, $m.line) `
        -Detail ("{0}: doc says {1}, register says {2}" -f $m.repo, $m.stated, $m.register)
    Write-Output ("    {0}:{1}  {2}: doc={3} register={4}" -f $m.file, $m.line, $m.repo, $m.stated, $m.register)
}

# ===================== check 4: count consistency ============================
Write-Output ''
Write-Output '== check 4: counts stated in prose vs reality =='
$reposCount = $repoNames.Count
Write-Output ("  actual reference repos: {0}" -f $reposCount)

$countClaims = @()
# Tightened after a false positive: the loose form matched the bare digits in
# "1800 files" and reported it as a repo-count claim. A count claim must have the
# number IMMEDIATELY before the repo keyword.
$countPattern = '(\d+)\s*(' + $GE + $CANKAO + $CANGKU + '|' + $CANKAO + $CANGKU + ')'
foreach ($f in $allMd) {
    $no = 0
    foreach ($line in (Get-Content -LiteralPath $f -Encoding UTF8 -ErrorAction SilentlyContinue)) {
        $no++
        foreach ($m in [regex]::Matches($line, $countPattern)) {
            $n = [int]$m.Groups[1].Value
            if ($n -ne $reposCount -and $n -gt 0) {
                $countClaims += [pscustomobject]@{
                    file = $f.Substring($root.Length).TrimStart('\'); line = $no
                    claimed = $n; actual = $reposCount
                    text = $line.Trim().Substring(0, [Math]::Min(90, $line.Trim().Length))
                }
            }
        }
    }
}
Write-Output ("  claims disagreeing with the actual repo count: {0}" -f $countClaims.Count)
foreach ($c in $countClaims) {
    Add-Finding -Kind 'count-mismatch' -Severity 'P2' -Where ("{0}:{1}" -f $c.file, $c.line) `
        -Detail ("claims {0} reference repos, actual {1}" -f $c.claimed, $c.actual)
    Write-Output ("    {0}:{1}  claims {2}, actual {3}" -f $c.file, $c.line, $c.claimed, $c.actual)
}

Write-Output ''
Write-Output '== check 5: structure =='
Write-Output '  delegated to tools/check-docs.ps1 -Strict'

# ===================== write the candidate list =============================
$p0 = @($findings | Where-Object { $_.severity -eq 'P0' })
$p1 = @($findings | Where-Object { $_.severity -eq 'P1' })
$p2 = @($findings | Where-Object { $_.severity -eq 'P2' })

$result = [pscustomobject]@{
    generated_at         = (Get-Date).ToString('s')
    note                 = 'CANDIDATE LIST, not a verdict. Task-book checks 1-2 need semantic judgement; this script cannot supply it.'
    files_scanned        = $allMd.Count
    section_refs_defined = $headings.Count
    dangling_refs        = @($dangling.Keys | Sort-Object)
    promise_candidates   = $promises
    licence_mismatches   = $mismatches
    count_mismatches     = $countClaims
    p0                   = $p0
    p1                   = $p1
    p2                   = $p2
}
$jsonPath = Join-Path $OutDir 'audit-candidates.json'
$result | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8

Write-Output ''
Write-Output '== candidate summary =='
Write-Output ("  P0 (wrong implementation or compliance risk): {0}" -f $p0.Count)
Write-Output ("  P1 (dangling reference / broken pointer):     {0}" -f $p1.Count)
Write-Output ("  P2 (cosmetic or count drift):                 {0}" -f $p2.Count)
Write-Output ("  promise candidates for human review:          {0}" -f $promises.Count)
Write-Output "report: $jsonPath"

# P0 alone decides the exit code: a licence mismatch is a compliance risk, while
# a dangling reference is a pointer to fix. Failing on P1/P2 too would make the
# audit red for reasons that do not block a milestone exit -- and an always-red
# audit stops being read.
if ($p0.Count -gt 0) { exit 1 }
exit 0
