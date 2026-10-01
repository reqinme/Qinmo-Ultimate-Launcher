#Requires -Version 5.1
<#
.SYNOPSIS
  Insert or replace a changelog row in a markdown table, keeping rows
  de-duplicated and ordered by version (newest first).

.DESCRIPTION
  WHY THIS EXISTS
  ---------------
  While editing docs I repeatedly inserted a new changelog row by replacing
  the anchor "| **vX.Y** |" with "<new row>\n<anchor>". That is wrong: the
  anchor string also matches the row I am inserting, so the file ends up with
  TWO rows of the same version. This happened three times.

  Mistake class: an anchor that matches the thing being inserted.
  The fix is not "be more careful" -- it is to stop hand-editing the table.

  This script:
    1. finds every changelog row in the file (lines matching "| **vN.N** |")
    2. drops any existing row for the given version
    3. inserts the new row in version order (numeric, newest first)
    4. reports what changed

  Independent of the edit, check-docs.ps1 should still be run afterwards.

.PARAMETER Path
  Markdown file to modify.

.PARAMETER Version
  Version label without the "v", e.g. "2.1". Matching is exact.

.PARAMETER RowFile
  File containing the complete new table row (single line, UTF-8).

.PARAMETER Reorder
  Also re-sort existing rows by version, newest first.

.EXAMPLE
  pwsh ./tools/upsert-changelog.ps1 -Path docs/UI.md -Version 2.1 -RowFile .git/ROW -Reorder
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Path,
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$RowFile,
  [switch]$Reorder
)

$ErrorActionPreference = 'Stop'

$resolved = (Resolve-Path -LiteralPath $Path).Path
$lines = [System.IO.File]::ReadAllLines($resolved, [Text.Encoding]::UTF8)
$row = ([System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $RowFile).Path, [Text.Encoding]::UTF8)).Trim()

if ($row -notmatch '^\| \*\*v\d+\.\d+\*\*') {
  throw "Row must start with '| **vN.N**', got: $($row.Substring(0, [Math]::Min(40, $row.Length)))"
}
if ($row -notmatch "^\| \*\*v$([regex]::Escape($Version))\*\*") {
  throw "Row version does not match -Version '$Version'"
}

$rx = '^\| \*\*v(\d+)\.(\d+)\*\*'

# Collect changelog rows with their numeric version.
$rows = New-Object System.Collections.ArrayList
for ($i = 0; $i -lt $lines.Count; $i++) {
  $m = [regex]::Match($lines[$i], $rx)
  if ($m.Success) {
    [void]$rows.Add([pscustomobject]@{
      Index = $i
      Line  = $lines[$i]
      Major = [int]$m.Groups[1].Value
      Minor = [int]$m.Groups[2].Value
    })
  }
}

if ($rows.Count -eq 0) { throw "No changelog rows found in $Path" }

$targetMajor, $targetMinor = $Version.Split('.') | ForEach-Object { [int]$_ }
$existing = @($rows | Where-Object { $_.Major -eq $targetMajor -and $_.Minor -eq $targetMinor })

# Build the final ordered row list: existing rows (minus the target version) + the new row.
$others = @($rows | Where-Object { -not ($_.Major -eq $targetMajor -and $_.Minor -eq $targetMinor) })
$newRow = [pscustomobject]@{ Major = $targetMajor; Minor = $targetMinor; Line = $row }
$all = @($others) + @($newRow)

$ordered = @($all | Sort-Object -Property `
  @{ Expression = { $_.Major }; Descending = $true }, `
  @{ Expression = { $_.Minor }; Descending = $true })

# Rewrite: keep the first row position as the insertion point, drop the rest,
# then write the ordered rows back into those positions.
$positions = @($rows | ForEach-Object { $_.Index })
$out = New-Object System.Collections.ArrayList
$out.AddRange($lines)

for ($k = 0; $k -lt $positions.Count; $k++) {
  if ($k -lt $ordered.Count) { $out[$positions[$k]] = $ordered[$k].Line }
  else { $out[$positions[$k]] = $null }   # marker for removal
}

$final = @($out | Where-Object { $null -ne $_ })
[System.IO.File]::WriteAllLines($resolved, $final, (New-Object System.Text.UTF8Encoding($false)))

$verb = if ($existing.Count -gt 0) { 'replaced' } else { 'inserted' }
Write-Host "  $verb v$Version  (rows: $($rows.Count) -> $($ordered.Count))"
Write-Host "  order: " + (($ordered | ForEach-Object { "v$($_.Major).$($_.Minor)" }) -join ' -> ')

$dup = $ordered | Group-Object { "v$($_.Major).$($_.Minor)" } | Where-Object { $_.Count -gt 1 }
if ($dup) { throw "Duplicate versions remain: $($dup.Name -join ', ')" }
