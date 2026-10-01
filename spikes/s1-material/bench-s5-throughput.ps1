# S5 part 1: throughput baseline across {official, mirror} x {concurrency 1,8,16,32}.
#
# THIS SCRIPT IS DELIBERATELY ASCII-ONLY (PowerShell 5.1 reads BOM-less .ps1 as
# ANSI; non-ASCII bytes become mojibake and break parsing).
#
# WHY A MATRIX AND NOT ONE NUMBER
#
# The task book's own warning: "concurrency on a cross-border constrained link
# often gives NO improvement, or makes things worse. Decide with data, not
# intuition."
#
# The subject is the REAL client jar of the current release, pulled with HTTP
# Range requests: N parallel connections, each a distinct slice. That is how a
# real multi-segment downloader behaves, so the number transfers.
#
# WHY A BYTE BUDGET INSTEAD OF THE WHOLE FILE
#
# The client jar is ~39.6 MB and the link may be slow; a 2 x 4 matrix over the
# whole file could take hours. Each cell pulls a fixed budget and reports MB/s.
# The budget is printed so these are never mistaken for install timings.
#
# WHY THE DOWNLOAD PATH CONTAINS NO POWERSHELL
#
# The first version called a PowerShell function from inside Task.Run and failed
# with "there is no Runspace available to run scripts in this thread" -- the .NET
# thread pool has no PowerShell runspace. The fix is not to add one but to keep
# the parallel section pure .NET; PowerShell only drives the matrix around it.
#
# Usage:
#   pwsh -File spikes/s1-material/bench-s5-throughput.ps1
#   pwsh -File spikes/s1-material/bench-s5-throughput.ps1 -BudgetMB 4 -Repeat 1 -Concurrency 1,8
# ---------------------------------------------------------------------------

param(
    [int]$BudgetMB = 6,
    [int]$Repeat = 3,
    [int[]]$Concurrency = @(1, 8, 16, 32),
    [string]$OutDir = '',
    [int]$TimeoutSec = 120
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http

$scriptDir# Anchor at the REPO ROOT by walking up until .git is found. The first version
# used "two levels up from this script", which is `spikes/` rather than the
# repo -- so its output went to `spikes/out/s5` while the sibling script looked
# under `spikes/s1-material/out/s5`. Deriving a root by counting parents drifts
# whenever a file moves; searching for a marker does not.
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
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $scriptDir 'out\s5' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

$assetPath = Join-Path $OutDir 'asset.json'
if (-not (Test-Path $assetPath)) { throw "missing $assetPath -- run find-download-asset.ps1 first" }
$asset = Get-Content $assetPath -Raw -Encoding UTF8 | ConvertFrom-Json

$official = $asset.client.url
$mirror = $asset.mirror_forms.client
$totalSize = [int64]$asset.client.size
$budget = [int64]$BudgetMB * 1MB

Write-Output ("subject  : client.jar of version {0}  ({1:N1} MB total)" -f $asset.version, ($totalSize / 1MB))
Write-Output ("official : {0}" -f $official)
Write-Output ("mirror   : {0}" -f $mirror)
Write-Output ("per cell : {0} MB, repeated {1}x, concurrency {2}" -f $BudgetMB, $Repeat, ($Concurrency -join '/'))
Write-Output ""

# Downloads one Range slice; returns the byte count. Pure .NET, no runspace needed.
$oneSlice = {
    param($Url, $From, $To, $TimeoutSec)
    $handler = New-Object System.Net.Http.HttpClientHandler
    $client = New-Object System.Net.Http.HttpClient($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSec)
    try {
        $req = New-Object System.Net.Http.HttpRequestMessage('Get', $Url)
        $req.Headers.Range = New-Object System.Net.Http.Headers.RangeHeaderValue($From, $To)
        $resp = $client.SendAsync($req, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
        if (-not $resp.IsSuccessStatusCode) { return -1 }
        $stream = $resp.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $buf = New-Object byte[] 131072
        $total = 0
        while ($true) {
            $n = $stream.Read($buf, 0, $buf.Length)
            if ($n -le 0) { break }
            $total += $n
        }
        $stream.Dispose()
        $resp.Dispose()
        return $total
    }
    catch { return -1 }
    finally { $client.Dispose(); $handler.Dispose() }
}

function Measure-Cell {
    param([string]$Label, [string]$Url, [int]$N, [int64]$Budget, [int64]$TotalSize, [int]$TimeoutSec)

    $per = [int64][Math]::Floor($Budget / $N)
    if ($per -lt 65536) { $per = 65536 }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $tasks = New-Object System.Collections.ArrayList
    for ($i = 0; $i -lt $N; $i++) {
        $from = [int64]($i * $per)
        $to = $from + $per - 1
        if ($to -ge $TotalSize) { $to = $TotalSize - 1 }
        if ($from -ge $TotalSize) { continue }
        # Invoke the slice script block and keep the pipeline as a Task, so all
        # slices start before any is awaited.
        $ps = [PowerShell]::Create()
        [void]$ps.AddScript($oneSlice.ToString()).AddArgument($Url).AddArgument($from).
            AddArgument($to).AddArgument($TimeoutSec)
        [void]$tasks.Add([pscustomobject]@{ ps = $ps; handle = $ps.BeginInvoke() })
    }

    $bytes = 0
    $errors = 0
    foreach ($t in $tasks) {
        try {
            $out = $t.ps.EndInvoke($t.handle)
            $val = [int64]($out | Select-Object -Last 1)
            if ($val -lt 0) { $errors++ } else { $bytes += $val }
        }
        catch { $errors++ }
        finally { $t.ps.Dispose() }
    }
    $sw.Stop()
    $secs = [Math]::Max(0.001, $sw.Elapsed.TotalSeconds)
    return [pscustomobject]@{
        label       = $Label
        concurrency = $N
        bytes       = $bytes
        seconds     = [Math]::Round($secs, 2)
        mb_per_s    = [Math]::Round((($bytes / 1MB) / $secs), 3)
        error_count = $errors
    }
}

$rows = @()
foreach ($pass in 1..$Repeat) {
    Write-Output ("---- pass {0}/{1} ----" -f $pass, $Repeat)
    foreach ($src in @(
            @{ label = 'official'; url = $official },
            @{ label = 'mirror'; url = $mirror }
        )) {
        foreach ($n in $Concurrency) {
            try {
                $cell = Measure-Cell -Label $src.label -Url $src.url -N $n -Budget $budget `
                    -TotalSize $totalSize -TimeoutSec $TimeoutSec
                $cell | Add-Member -NotePropertyName pass -NotePropertyValue $pass
                $rows += $cell
                Write-Output ("  {0,-9} conc {1,2} -> {2,7:N3} MB/s   {3,6:N1} MB in {4,6:N1} s   errors={5}" -f `
                        $src.label, $n, $cell.mb_per_s, ($cell.bytes / 1MB), $cell.seconds, $cell.error_count)
            }
            catch {
                Write-Output ("  {0,-9} conc {1,2} -> FAILED: {2}" -f $src.label, $n, $_.Exception.Message)
            }
        }
    }
}

function Get-Median {
    param($Values)
    $v = @($Values | Sort-Object)
    if ($v.Count -eq 0) { return 0 }
    if ($v.Count % 2 -eq 1) { return $v[[int]($v.Count / 2)] }
    return (($v[$v.Count / 2 - 1] + $v[$v.Count / 2]) / 2)
}

Write-Output ""
Write-Output "==== S5 throughput matrix (median MB/s over passes) ===="
$header = "  {0,-10}" -f 'source'
foreach ($n in $Concurrency) { $header += (" {0,9}" -f "conc $n") }
Write-Output $header
$summary = @{}
foreach ($label in @('official', 'mirror')) {
    $line = "  {0,-10}" -f $label
    foreach ($n in $Concurrency) {
        $vals = @($rows | Where-Object { $_.label -eq $label -and $_.concurrency -eq $n } | ForEach-Object { $_.mb_per_s })
        $m = Get-Median $vals
        $summary["$label-$n"] = $m
        $line += (" {0,9:N3}" -f $m)
    }
    Write-Output $line
}

Write-Output ""
Write-Output "==== conclusions ===="
foreach ($label in @('official', 'mirror')) {
    $best = [pscustomobject]@{ conc = 0; mb = 0.0 }
    foreach ($n in $Concurrency) {
        $m = $summary["$label-$n"]
        if ($m -gt $best.mb) { $best = [pscustomobject]@{ conc = $n; mb = $m } }
    }
    $c1 = $summary["$label-1"]
    $gain = if ($c1 -gt 0) { $best.mb / $c1 } else { 0 }
    Write-Output ("  {0,-9}: best concurrency {1,2} at {2,8:N3} MB/s   (x{3:N2} vs a single connection)" -f `
            $label, $best.conc, $best.mb, $gain)
}
$off1 = $summary['official-1']
$mir1 = $summary['mirror-1']
if ($off1 -gt 0 -and $mir1 -gt 0) {
    $ratio = $mir1 / $off1
    $word = if ($ratio -ge 1.0) { 'FASTER' } else { 'SLOWER' }
    Write-Output ("  mirror vs official at concurrency 1: {0:N2}x ({1})" -f $ratio, $word)
}

$out = [pscustomobject]@{
    generated_at = (Get-Date).ToString('s')
    version      = $asset.version
    official_url = $official
    mirror_url   = $mirror
    budget_mb    = $BudgetMB
    repeat       = $Repeat
    concurrency  = $Concurrency
    notes        = 'Each cell pulls budget_mb across N parallel Range connections. NOT whole-file install timings.'
    rows         = $rows
    medians      = $summary
}
$jsonPath = Join-Path $OutDir 's5-throughput.json'
$out | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding UTF8
Write-Output ""
Write-Output "report: $jsonPath"
