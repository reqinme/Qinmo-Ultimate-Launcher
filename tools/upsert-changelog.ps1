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

if ($row -notmatch '^>? ?\| \*\*v\d+\.\d+\*\*') {
  throw "Row must start with '| **vN.N**', got: $($row.Substring(0, [Math]::Min(40, $row.Length)))"
}
if ($row -notmatch "^>? ?\| \*\*v$([regex]::Escape($Version))\*\*") {
  throw "Row version does not match -Version '$Version'"
}

$rx = '^>? ?\| \*\*v(\d+)\.(\d+)\*\*'
# Guard against MALFORMED changelog row lines.
#
# WHY (this destroyed the v1.3 row once): a bad edit turned a changelog row into a
# line that still began with "**v1.3** | ..." but had LOST its leading pipe. Such a
# line does NOT match $rx (which requires a leading "|"), so it was invisible to
# both the dedup pass and the rewrite pass -- and got silently dropped.
#
# So detection and matching must use DIFFERENT patterns:
#   * $loose  -- "does this line look like a changelog row?"  (broad)
#   * $rx     -- "is this a well-formed row I may rewrite?"   (strict)
# Anything that looks like a row but is not well-formed stops the script.
$loose = 'v\d+\.\d+\*\*\s*\|'
$malformed = @()
for ($i = 0; $i -lt $lines.Count; $i++) {
  if ($lines[$i] -notmatch $loose) { continue }
  if ([regex]::Match($lines[$i], $rx).Success) { continue }
  $malformed += [pscustomobject]@{ Index = $i; Line = $lines[$i] }
}
if ($malformed.Count -gt 0) {
  Write-Host "  MALFORMED changelog row(s) - refusing to rewrite:"
  foreach ($m in $malformed) {
    $preview = $m.Line.Trim()
    if ($preview.Length -gt 70) { $preview = $preview.Substring(0, 70) + '...' }
    Write-Host "    line $($m.Index + 1): $preview"
  }
  throw "Fix these lines by hand so they start and end with '|', then re-run."
}

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

# Parse the version explicitly. Do NOT use:
#     $targetMajor, $targetMinor = $Version.Split('.') | ForEach-Object { [int]$_ }
# That form is NOT reliable: on Windows PowerShell 5.1 it can bind the WHOLE array
# to $targetMajor, which makes "$_.Major -eq $targetMajor" always false.
# Symptom: "existing = 0", so the script INSERTED a second row of the same version
# instead of REPLACING the old one -- and then reported "Duplicate versions remain".
$parts = @($Version.Split('.'))
if ($parts.Count -ne 2) { throw "-Version must look like 'N.N', got '$Version'" }
$targetMajor = [int]$parts[0]
$targetMinor = [int]$parts[1]

# Deduplicate by version, keeping the FIRST occurrence of each version.
# (Defensive: a file may already contain duplicates from an earlier bad edit.)
#
# $allSlots keeps the ORIGINAL physical line positions. The rewrite below needs
# these, not the deduplicated list -- see the comment at the rewrite step.
$allSlots = @($rows)
$seen = @{}
$dedup = New-Object System.Collections.ArrayList
foreach ($r in $rows) {
  $key = "$($r.Major).$($r.Minor)"
  if ($seen.ContainsKey($key)) { continue }
  $seen[$key] = $true
  [void]$dedup.Add($r)
}
$rows = $dedup

$existing = @($rows | Where-Object { $_.Major -eq $targetMajor -and $_.Minor -eq $targetMinor })

# Build the final ordered row list: existing rows (minus the target version) + the new row.
$others = @($rows | Where-Object { -not ($_.Major -eq $targetMajor -and $_.Minor -eq $targetMinor) })
$newRow = [pscustomobject]@{ Major = $targetMajor; Minor = $targetMinor; Line = $row }
$all = @($others) + @($newRow)

$ordered = @($all | Sort-Object -Property `
  @{ Expression = { $_.Major }; Descending = $true }, `
  @{ Expression = { $_.Minor }; Descending = $true })

# Rewrite the version-row block.
#
# THE BUG THIS FIXES (found by debugging, twice):
#   The original loop iterated over the *deduplicated* row list:
#       for ($k = 0; $k -lt $positions.Count; $k++) { ... }
#   When a duplicate was dropped, the deduplicated list was SHORTER than the set of
#   physical rows, so the surplus physical rows were never visited:
#     * the duplicate row's line was never overwritten  -> it survived
#     * the last row's line was never blanked           -> count never decreased
#   Symptom: the script reported "rows: 8 -> 8, no duplicates" while the file still
#   contained two rows of the same version. The report was computed from the
#   in-memory list, so it agreed with itself and disagreed with the file.
#
# FIX: keep the ORIGINAL physical row positions ($slots), blank out EVERY slot,
# then fill the first $ordered.Count slots. Slots beyond that stay blank and are
# removed. Now the write is driven by physical layout, not by the logical list.
$slots = @($allSlots | ForEach-Object { $_.Index })
$removed = [Math]::Max(0, $slots.Count - $ordered.Count)
Write-Host "  slots=$($slots.Count)  ordered=$($ordered.Count)  removed=$removed"
$out = New-Object System.Collections.ArrayList
$out.AddRange($lines)

foreach ($p in $slots) { $out[$p] = $null }        # blank every version-row slot
for ($k = 0; $k -lt $ordered.Count; $k++) {        # refill from the top
  if ($k -lt $slots.Count) { $out[$slots[$k]] = $ordered[$k].Line }
}

$final = @($out | Where-Object { $null -ne $_ })
[System.IO.File]::WriteAllLines($resolved, $final, (New-Object System.Text.UTF8Encoding($false)))

$verb = if ($existing.Count -gt 0) { 'replaced' } else { 'inserted' }
Write-Host "  $verb v$Version  (rows: $($rows.Count) -> $($ordered.Count))"
Write-Host "  order: " + (($ordered | ForEach-Object { "v$($_.Major).$($_.Minor)" }) -join ' -> ')

$dup = $ordered | Group-Object { "v$($_.Major).$($_.Minor)" } | Where-Object { $_.Count -gt 1 }
if ($dup) { throw "Duplicate versions remain: $($dup.Name -join ', ')" }
