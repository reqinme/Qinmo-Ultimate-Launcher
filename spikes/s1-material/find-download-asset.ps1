#!/usr/bin/env pwsh
# Discover a REAL downloadable asset per source, so throughput tests measure
# something that exists rather than a guess.
#
# M0 spike S5 needs a big file to time. The obvious candidates (the roots
# libraries.minecraft.net / resources.download.minecraft.net) are 404 -- they are
# bucket roots, not browsable indexes. This walks the official metadata instead:
#
#   version_manifest_v2.json  ->  <version>.json  ->  downloads.client.url
#
# and also builds the BMCLAPI mirror form of the same URL, so both sides of the
# comparison are the same bytes.
#
# Output: JSON on stdout. It is cached to out/s5/asset.json so the later
# benchmarks do not re-fetch metadata on every run.
#
# Usage:
#   pwsh -File spikes/s1-material/find-download-asset.ps1
#   pwsh -File spikes/s1-material/find-download-asset.ps1 -Force

param(
    [string]$Version = '',                 # e.g. 1.21.4; default = latest release
    [string]$OutDir = '',
    [switch]$Force,
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = 'Stop'

# Anchor at the REPO ROOT, not at "two levels up from this script": two levels up
# is `spikes/`, not the repo, which is why the first run wrote its output to
# `spikes/out/s5` and the later run then could not find `asset.json`.
# Deriving it from the script's own location minus a fixed count of parents was
# the bug; asking git where the root is cannot drift when files move.
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = $scriptDir
while ($root -and -not (Test-Path (Join-Path $root '.git'))) {
    $parent = Split-Path -Parent $root
    if ($parent -eq $root -or [string]::IsNullOrEmpty($parent)) { break }
    $root = $parent
}
if (-not $root -or -not (Test-Path (Join-Path $root 'Cargo.toml'))) {
    throw "cannot locate the repo root from $scriptDir"
}
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $root 'spikes\s1-material\out\s5' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$cache = Join-Path $OutDir 'asset.json'
if ((Test-Path $cache) -and -not $Force) {
    Write-Output (Get-Content $cache -Raw -Encoding UTF8)
    exit 0
}

$officialManifest = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'

Write-Host "fetching official version manifest ..." -ForegroundColor DarkGray
$manifest = Invoke-RestMethod -Uri $officialManifest -TimeoutSec $TimeoutSec

$chosen = $null
if ([string]::IsNullOrWhiteSpace($Version)) {
    $id = $manifest.latest.release
    $chosen = $manifest.versions | Where-Object { $_.id -eq $id } | Select-Object -First 1
}
else {
    $chosen = $manifest.versions | Where-Object { $_.id -eq $Version } | Select-Object -First 1
}
if (-not $chosen) { throw "version not found in the manifest: '$Version'" }

Write-Host ("  version {0}  ({1})" -f $chosen.id, $chosen.type) -ForegroundColor DarkGray
$detail = Invoke-RestMethod -Uri $chosen.url -TimeoutSec $TimeoutSec

$client = $detail.downloads.client
if (-not $client) { throw "version $($chosen.id) has no client download" }

# The BMCLAPI mirror keeps the same path but replaces the host. Verified shape:
#   https://piston-meta.mojang.com/...            -> metadata
#   https://bmclapi2.bangbang93.com/...           -> that host's form
# libraries.minecraft.net and resources.download.minecraft.net have dedicated
# BMCLAPI hosts, which are what a launcher would actually switch to.
function Mirror-Url {
    param([string]$Url)
    $u = $Url
    $u = $u -replace '^https://libraries\.minecraft\.net/', 'https://bmclapi2.bangbang93.com/maven/'
    $u = $u -replace '^https://resources\.download\.minecraft\.net/', 'https://bmclapi2.bangbang93.com/assets/'
    $u = $u -replace '^https://piston-data\.mojang\.com/', 'https://bmclapi2.bangbang93.com/'
    $u = $u -replace '^https://piston-meta\.mojang\.com/', 'https://bmclapi2.bangbang93.com/'
    return $u
}

# Pick a LIBRARY jar as the throughput subject rather than the client jar:
# it is a few MB instead of tens of MB, so a 3-source x 3-concurrency matrix stays
# affordable, while still being a real file that a real install must download.
$libSample = $null
foreach ($lib in $detail.libraries) {
    $artifact = $lib.downloads.artifact
    if ($artifact -and $artifact.url -and $artifact.size) {
        if ($artifact.size -ge 500000 -and $artifact.size -le 8000000) {
            $libSample = $artifact
            break
        }
    }
}

$result = [ordered]@{
    generated_at    = (Get-Date).ToString('s')
    version         = $chosen.id
    version_type    = $chosen.type
    version_url     = $chosen.url
    client = [ordered]@{
        path = $client.path
        url  = $client.url
        size = $client.size
        sha1 = $client.sha1
    }
    library_sample = if ($libSample) {
        [ordered]@{
            path = $libSample.path
            url  = $libSample.url
            size = $libSample.size
            sha1 = $libSample.sha1
        }
    } else { $null }
    mirror_forms = [ordered]@{
        client     = (Mirror-Url $client.url)
        lib_sample = if ($libSample) { (Mirror-Url $libSample.url) } else { $null }
    }
}

$json = $result | ConvertTo-Json -Depth 6
$json | Set-Content -Path $cache -Encoding UTF8
Write-Output $json
