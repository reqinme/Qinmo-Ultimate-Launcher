# =============================================================================
# analyse.ps1 -- find WHERE the version-JSON structure changes across 17 years
#
# WHAT QUESTION THIS ANSWERS:
#
#   M2 has to parse version JSONs from 2009 to 2026. The task book calls this
#   "cross-era structural differences". The useful output is NOT "here are 17
#   files" -- it is "the structure changes at THESE boundaries, and here is
#   what changed at each one".
#
#   A parser writer needs the second thing. The first thing is a pile.
#
# HOW IT WORKS:
#
#   For each sampled version, report the presence/shape of the fields that a
#   launcher actually consumes. Then diff the SHAPE between consecutive
#   versions in chronological order, and print only the differences.
#
#   Diffing consecutive chronologically-ordered samples is what makes the
#   boundaries visible. Reporting 17 independent field lists would make the
#   reader do that comparison in their head -- which is exactly the kind of
#   "looks about right" the task book forbids.
#
# NOTE (project rule): this file must stay ASCII-only.
#
# Usage: pwsh -File spikes/m2-metadata-shapes/analyse.ps1
# =============================================================================

$ErrorActionPreference = 'Stop'

$rawDir = Join-Path $PSScriptRoot 'raw'
$files = Get-ChildItem $rawDir -Filter '*.json' | ForEach-Object { $_.Name }
if ($files.Count -eq 0) { Write-Output 'FAIL: no samples; run sample.ps1 first'; exit 2 }

# Chronological order matters for the diff. Read releaseTime out of each file.
$items = @()
foreach ($f in $files) {
    $o = Get-Content (Join-Path $rawDir $f) -Raw -Encoding UTF8 | ConvertFrom-Json
    $items += [pscustomobject]@{ id = $o.id; time = $o.releaseTime; json = $o }
}
$sorted = $items | Sort-Object { [datetime]$_.time }

# -----------------------------------------------------------------------------
# The "shape" of one version JSON, as a flat ordered map of field -> value-shape.
#
# Only fields a launcher consumes. Naming them here (rather than dumping every
# key) is the difference between an analysis and a transcript.
# -----------------------------------------------------------------------------
function Shape-Of {
    param($o)

    $has = { param($n) return ($null -ne $o.PSObject.Properties[$n]) }

    # arguments: absent / only game / both jvm+game
    $argShape = 'ABSENT'
    if (& $has 'arguments') {
        $jvm = $null -ne $o.arguments.PSObject.Properties['jvm']
        $game = $null -ne $o.arguments.PSObject.Properties['game']
        $argShape = if ($jvm -and $game) { 'jvm+game' } elseif ($game) { 'game-only' } elseif ($jvm) { 'jvm-only' } else { 'empty' }
    }

    $mcArgs = if (& $has 'minecraftArguments') { 'string' } else { 'ABSENT' }

    # libraries: which download shapes appear
    $libShapes = @()
    if (& $has 'libraries') {
        $hasClassifiers = $false; $hasNatives = $false; $hasRules = $false; $hasExtract = $false
        foreach ($l in $o.libraries) {
            if ($null -ne $l.downloads.PSObject.Properties['classifiers']) { $hasClassifiers = $true }
            if ($null -ne $l.PSObject.Properties['natives']) { $hasNatives = $true }
            if ($null -ne $l.PSObject.Properties['rules']) { $hasRules = $true }
            if ($null -ne $l.PSObject.Properties['extract']) { $hasExtract = $true }
        }
        if ($hasClassifiers) { $libShapes += 'classifiers' }
        if ($hasNatives) { $libShapes += 'natives' }
        if ($hasRules) { $libShapes += 'rules' }
        if ($hasExtract) { $libShapes += 'extract' }
    }
    $libShape = if ($libShapes.Count -eq 0) { 'ABSENT' } else { $libShapes -join '+' }

    # assets: named index vs bare id vs absent
    $assetShape = 'ABSENT'
    if (& $has 'assetIndex') { $assetShape = "assetIndex($($o.assetIndex.id))" }
    elseif (& $has 'assets') { $assetShape = "assets-string($($o.assets))" }

    $javaShape = if (& $has 'javaVersion') { "javaVersion($($o.javaVersion.majorVersion))" } else { 'ABSENT' }
    $loggingShape = if (& $has 'logging') { 'present' } else { 'ABSENT' }
    $compliance = if (& $has 'complianceLevel') { $o.complianceLevel } else { 'ABSENT' }
    $dlShape = if (& $has 'downloads') {
        $k = @($o.downloads.PSObject.Properties.Name)
        $k -join '+'
    } else { 'ABSENT' }
    $mainClass = if (& $has 'mainClass') { $o.mainClass } else { 'ABSENT' }
    $minLauncher = if (& $has 'minimumLauncherVersion') { $o.minimumLauncherVersion } else { 'ABSENT' }

    return [ordered]@{
        key_count        = @($o.PSObject.Properties.Name).Count
        arguments        = $argShape
        minecraftArgs    = $mcArgs
        libraries        = $libShape
        lib_count        = if (& $has 'libraries') { @($o.libraries).Count } else { 0 }
        assets           = $assetShape
        javaVersion      = $javaShape
        logging          = $loggingShape
        complianceLevel  = $compliance
        downloads        = $dlShape
        mainClass        = $mainClass
        minimumLauncher  = $minLauncher
    }
}

# Build the shape table.
$rows = @()
foreach ($it in $sorted) {
    $rows += [pscustomobject]@{ id = $it.id; time = ([datetime]$it.time).ToString('yyyy-MM-dd'); shape = (Shape-Of $it.json) }
}

Write-Output '== version JSON shape, oldest to newest =='
Write-Output ''
$fields = @('key_count','arguments','minecraftArgs','libraries','lib_count','assets','javaVersion','logging','complianceLevel','downloads','mainClass','minimumLauncher')
$hdr = '  {0,-14} {1,-11} ' -f 'id', 'released'
foreach ($fl in $fields) { $hdr += ('{0,-16}' -f $fl) }
Write-Output $hdr
foreach ($r in $rows) {
    $line = '  {0,-14} {1,-11} ' -f $r.id, $r.time
    foreach ($fl in $fields) { $line += ('{0,-16}' -f ([string]$r.shape[$fl])) }
    Write-Output $line
}

Write-Output ''
Write-Output '== where the shape CHANGES (consecutive diff) =='
Write-Output ''
$boundaries = 0
for ($i = 1; $i -lt $rows.Count; $i++) {
    $a = $rows[$i - 1]
    $b = $rows[$i]
    $diffs = @()
    foreach ($fl in $fields) {
        $va = [string]$a.shape[$fl]
        $vb = [string]$b.shape[$fl]
        if ($va -ne $vb) { $diffs += ('{0}: {1} -> {2}' -f $fl, $va, $vb) }
    }
    if ($diffs.Count -gt 0) {
        $boundaries++
        Write-Output ("  {0}  ->  {1}" -f $a.id, $b.id)
        foreach ($d in $diffs) { Write-Output ("      {0}" -f $d) }
    }
}
Write-Output ''
Write-Output ("  boundaries found: {0} (across {1} samples)" -f $boundaries, $rows.Count)
Write-Output ''
Write-Output 'NOTE: this is a SAMPLE, not all 917 versions. A boundary reported here'
Write-Output '      is real; a boundary NOT reported here may still exist between two'
Write-Output '      samples. The sample index records why each version was picked.'
exit 0
