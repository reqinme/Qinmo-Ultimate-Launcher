# =============================================================================
# sample.ps1 -- download a REPRESENTATIVE sample of version JSONs for analysis
#
# WHY A SAMPLE AND NOT ALL 917:
#
#   The manifest lists 917 versions spanning 2009-2026. Downloading all of them
#   would be ~40 MB and would not answer the question. The question M2 has to
#   answer is "where does the STRUCTURE change?" -- and structure changes at a
#   handful of boundaries, not once per version.
#
#   So this picks versions at the boundaries and inside each era, and records
#   WHY each one was picked. A sample whose selection rule is not written down
#   is just a pile of files.
#
# WHAT IT STORES:
#
#   raw JSON, byte-for-byte, keyed by version id. NOT parsed and re-serialised
#   -- the raw bytes are the evidence, and re-serialising would destroy field
#   ORDER, which is itself something we need to observe (see the analysis).
#
# NOTE (project rule): this file must stay ASCII-only.
#
# Usage:
#   pwsh -File spikes/m2-metadata-shapes/sample.ps1
# Exit 0 = every requested version was fetched. A missing one is a FAILURE, not
# a skip: an absent file would silently shrink the evidence.
# =============================================================================

$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot
while ($root -and -not (Test-Path (Join-Path $root '.git'))) {
    $parent = Split-Path $root -Parent
    if ($parent -eq $root) { $root = $null } else { $root = $parent }
}
if (-not $root) { Write-Output 'FAIL: no repo root'; exit 2 }

$rawDir = Join-Path $PSScriptRoot 'raw'
if (-not (Test-Path $rawDir)) { New-Item -ItemType Directory -Path $rawDir -Force | Out-Null }

$manifestPath = Join-Path $env:APPDATA '.minecraft\versions\version_manifest_v2.json'
if (-not (Test-Path $manifestPath)) {
    Write-Output "FAIL: no version manifest at $manifestPath"
    exit 2
}
$manifest = Get-Content $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json

# -----------------------------------------------------------------------------
# The sample, with the reason for each pick.
#
# `why` is not decoration: when the analysis later says "structure changed
# between X and Y", the reader needs to know the two versions were chosen to
# bracket a suspected boundary, not at random.
# -----------------------------------------------------------------------------
$picks = @(
    @{ id = '26.3';            why = 'current latest release (2026)' }
    @{ id = '1.21.4';          why = 'modern release; still the new natives form at 51 entries' }
    @{ id = '1.19.3';          why = 'natives moved from downloads.classifiers to standalone entries (found by this analysis, not assumed)' }
    @{ id = '1.18.2';          why = 'last release BEFORE the natives change (16 old-style entries)' }
    @{ id = '1.17.1';          why = 'first release requiring Java 16+' }
    @{ id = '1.16.5';          why = 'post-arguments-rewrite, pre-natives-change; old-style natives still 16 entries' }
    @{ id = '1.14.4';          why = 'first release with client_mappings in downloads' }
    @{ id = '1.13.2';          why = 'THE argument-format boundary: minecraftArguments -> arguments.jvm/game' }
    @{ id = '1.12.2';          why = 'the most-modded old version; classic layout' }
    @{ id = '1.11.2';          why = 'mid-old' }
    @{ id = '1.8.9';           why = 'last release before downloads dropped windows_server' }
    @{ id = '1.7.10';          why = 'the classic modding baseline' }
    @{ id = '1.6.4';           why = 'the ONLY sample with no javaVersion and no complianceLevel; 11 keys' }
    @{ id = '1.5.2';           why = 'very old launcher-era layout' }
    @{ id = 'b1.7.3';          why = 'beta' }
    @{ id = 'a1.2.6';          why = 'alpha' }
    @{ id = 'rd-132211';       why = 'the OLDEST entry in the manifest' }
)

$ok = 0
$fail = @()
foreach ($p in $picks) {
    $v = $manifest.versions | Where-Object { $_.id -eq $p.id } | Select-Object -First 1
    if (-not $v) {
        $fail += "$($p.id) : not in the manifest"
        continue
    }
    if (-not $v.url) {
        $fail += "$($p.id) : manifest entry has no url"
        continue
    }
    $dest = Join-Path $rawDir ("$($p.id).json")
    if (Test-Path $dest) {
        Write-Output ("  cached   {0,-14} {1,6} bytes" -f $p.id, (Get-Item $dest).Length)
        $ok++
        continue
    }
    try {
        # Write TEXT, not bytes. The first version called WriteAllBytes with
        # `$resp.Content`, which in PowerShell 7 is a STRING -- so the call threw
        # and PowerShell then dumped the whole JSON into the error message,
        # flooding 24 KB per failure. Two lessons in one:
        #   1. match the API to the type (Content is a string here);
        #   2. an error path that echoes the payload is a bad error path.
        $resp = Invoke-WebRequest -Uri $v.url -TimeoutSec 30 -UseBasicParsing
        $text = if ($resp.Content -is [byte[]]) {
            [System.Text.Encoding]::UTF8.GetString($resp.Content)
        } else {
            [string]$resp.Content
        }
        [System.IO.File]::WriteAllText($dest, $text, (New-Object System.Text.UTF8Encoding($false)))
        Write-Output ("  fetched  {0,-14} {1,6} chars" -f $p.id, $text.Length)
        $ok++
    } catch {
        # Report the URL and the exception TYPE, never the payload.
        $fail += ("{0} : {1}: {2}" -f $p.id, $_.Exception.GetType().Name,
                  $_.Exception.Message.Split([char]10)[0])
    }
    Start-Sleep -Milliseconds 150
}

Write-Output ''
Write-Output ("  fetched/cached : {0} / {1}" -f $ok, $picks.Count)
if ($fail.Count -gt 0) {
    Write-Output 'FAILED (a missing sample silently shrinks the evidence):'
    $fail | ForEach-Object { Write-Output ("    {0}" -f $_) }
    exit 1
}

# Write the selection rule down next to the data, so the analysis can cite it.
#
# NOTE: the first version of this block computed `type` with
# `$manifest.versions | Where-Object { $_.id -eq $_.id }` -- a tautology that
# always matches the first entry, so every row would have claimed the type of
# whatever happened to be first. It is resolved properly here instead, from the
# manifest entry we already looked up.
$index = @()
foreach ($p in $picks) {
    $v = $manifest.versions | Where-Object { $_.id -eq $p.id } | Select-Object -First 1
    $f = Join-Path $rawDir ("$($p.id).json")
    $index += [pscustomobject]@{
        id           = $p.id
        why          = $p.why
        type         = if ($v) { $v.type } else { '(not found)' }
        compliance   = if ($v) { $v.complianceLevel } else { -1 }
        release_time = if ($v) { $v.releaseTime } else { '' }
        bytes        = if (Test-Path $f) { (Get-Item $f).Length } else { 0 }
    }
}
$idxPath = Join-Path $PSScriptRoot 'sample-index.json'
[System.IO.File]::WriteAllText($idxPath, ($index | ConvertTo-Json -Depth 4), (New-Object System.Text.UTF8Encoding($false)))
Write-Output ("  wrote {0}" -f $idxPath)
Write-Output 'OK'
exit 0
