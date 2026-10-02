#requires -Version 5.1
<#
  Shell contract check: the spec's machine tables  ==  the code.
  ============================================================================

  ## What it checks

  | Spec table (docs/UI...)                     | Code                                             |
  |---------------------------------------------|--------------------------------------------------|
  | 4.1.1  status cells (key / name / resident)  | web/src/routes/Shell.tsx  STATUS_ITEMS           |
  | 4.1.1  shell sizes (var / scope / value)     | web/src/routes/Shell.css  custom-property decls  |
  | 4.6.1  seven primary nav items               | web/src/routes/nav.ts PRIMARY + PRODUCT/TOOL_KEYS|
  | 4.6.1  the path of each primary              | web/src/routes/Shell.tsx  hrefOf()               |
  | 4.6.1.1 secondary sections + their items     | web/src/routes/nav.ts PRIMARY[*].secondary       |
  | (none) version shown on screen               | version.ts == package.json == tauri.conf.json    |

  ## Why this file is ASCII-only

  PowerShell 5.1 decodes a .ps1 by the **system code page** (936 on this box),
  so a Chinese literal written into this file comes back as mojibake --
  and that has already produced a bogus "missing file" error once
  (see tools/check-titlebar-contract.ps1).  So:

  * the spec file NAME is assembled from code points;
  * every expected VALUE is a raw string **compared**, never written here --
    the spec cell is the expected side, the code literal is the actual side;
  * the enumerations the script must *interpret* are ASCII in the spec
    (`yes`/`no`, `product`/`tool`, `narrow`/`default`).

  ## Why the tables are located by their first DATA row

  Anchoring on a heading or a column header would need Chinese.  Every machine
  table here has a first data row whose cells are backticked identifiers
  (``| `version` | ...``, ``| `home` | ...``), so the script can find it with
  pure ASCII and take the header from two lines above.

  ## The drift guards (a check that silently skips a row is worse than no check)

  * a line inside a parsed block that looks like an entry but is not understood
    is a FAILURE, not something to skip;
  * the number of parsed rows must equal the number of table rows, in order;
  * every variable named in the size table must have exactly one declaration
    per scope -- an extra `[data-rail=...]` override shows up as a failure.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Failures = New-Object System.Collections.Generic.List[string]

function Fail([string]$msg) {
  $script:Failures.Add($msg) | Out-Null
}

function Check([bool]$ok, [string]$msg) {
  if (-not $ok) { Fail $msg }
}

function Read-TextLines([string]$path) {
  return [System.IO.File]::ReadAllLines($path, [System.Text.Encoding]::UTF8)
}

# --- locate the repository root (walk up until .git is found) ----------------

$root = $PSScriptRoot
while (-not (Test-Path (Join-Path $root '.git'))) {
  $parent = Split-Path -Parent $root
  if ([string]::IsNullOrEmpty($parent) -or $parent -eq $root) { break }
  $root = $parent
}
if (-not (Test-Path (Join-Path $root '.git'))) {
  Write-Host 'FAIL: could not locate the repository root (.git not found above this script)'
  exit 1
}

# The spec file name is Chinese -- assemble it from code points (see header).
$specName = 'UI' + [char]0x8BBE + [char]0x8BA1 + [char]0x89C4 + [char]0x683C + '.md'
$specPath = Join-Path (Join-Path $root 'docs') $specName
if (-not (Test-Path $specPath)) {
  Write-Host "FAIL: spec file not found: $specPath"
  exit 1
}

$specLines = Read-TextLines $specPath
$shellLines = Read-TextLines (Join-Path $root 'web\src\routes\Shell.tsx')
$navLines = Read-TextLines (Join-Path $root 'web\src\routes\nav.ts')
$cssPath = Join-Path $root 'web\src\routes\Shell.css'

# --- table helpers -----------------------------------------------------------

function Split-Row([string]$line) {
  $t = $line.Trim()
  if ($t.StartsWith('|')) { $t = $t.Substring(1) }
  if ($t.EndsWith('|')) { $t = $t.Substring(0, $t.Length - 1) }
  $cells = @()
  foreach ($c in ($t -split '\|')) { $cells += $c.Trim() }
  return $cells
}

function Strip-Ticks([string]$cell) {
  return ($cell -replace '`', '').Trim()
}

function Find-Table($lines, [string]$firstRowPattern, [string]$what) {
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match $firstRowPattern) {
      if ($i -lt 2) {
        Fail "$what : the table has no header above its first row"
        return $null
      }
      if ($lines[$i - 1].Trim() -notmatch '^\|[\s\-:|]+\|$') {
        Fail "$what : the line above the first data row is not a table separator"
        return $null
      }
      $rows = @()
      $j = $i
      while ($j -lt $lines.Count -and $lines[$j].Trim().StartsWith('|')) {
        $rows += , (Split-Row $lines[$j])
        $j++
      }
      return @{ FirstLine = $i + 1; Rows = $rows }
    }
  }
  Fail "$what : table not found (first-row pattern: $firstRowPattern)"
  return $null
}

function Extract-Block($lines, [string]$startPattern, [string]$endPattern, [string]$what) {
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match $startPattern) {
      $inner = @()
      for ($j = $i + 1; $j -lt $lines.Count; $j++) {
        if ($lines[$j] -match $endPattern) {
          return @{ Lines = $inner; StartLine = $i + 1; EndLine = $j + 1 }
        }
        $inner += , @{ Text = $lines[$j]; No = $j + 1 }
      }
      Fail "$what : block starting at line $($i + 1) is never closed"
      return $null
    }
  }
  Fail "$what : block not found ($startPattern)"
  return $null
}

# --- spec tables -------------------------------------------------------------

$specStatus = Find-Table $specLines '^\| `version` \|' 'spec 4.1.1 status cells'
$specSizes = Find-Table $specLines '^\| `--shell-rail-w` \| narrow \|' 'spec 4.1.1 shell sizes'
$specNav = Find-Table $specLines '^\| `home` \|' 'spec 4.6.1 primary nav'
$specSections = Find-Table $specLines '^\| `instances` \| `product` \|' 'spec 4.6.1.1 sections'
$specItems = Find-Table $specLines '^\| `instances` \| `group` \| `all` \|' 'spec 4.6.1.1 items'

foreach ($t in @($specStatus, $specSizes, $specNav, $specSections, $specItems)) {
  if ($null -eq $t) {
    Write-Host 'FAIL: the spec tables are not in the shape this script reads -- nothing compared.'
    foreach ($f in $script:Failures) { Write-Host "  - $f" }
    exit 1
  }
}

# --- code: STATUS_ITEMS ------------------------------------------------------

$statusBlock = Extract-Block $shellLines 'const STATUS_ITEMS: readonly StatusItemSpec\[\] = \[' '^\];' 'Shell.tsx STATUS_ITEMS'
$codeStatus = @()
if ($null -ne $statusBlock) {
  foreach ($l in $statusBlock.Lines) {
    if ($l.Text -match '^\s*\{\s*key:\s*"([^"]+)",\s*label:\s*"([^"]+)",\s*resident:\s*(true|false)\s*\},?\s*$') {
      $codeStatus += , @{ Key = $Matches[1]; Label = $Matches[2]; Resident = ($Matches[3] -eq 'true'); No = $l.No }
    }
    elseif ($l.Text -match '^\s*\{') {
      Fail "Shell.tsx:$($l.No): a STATUS_ITEMS entry this script does not understand: $($l.Text.Trim())"
    }
  }
}

if ($null -ne $statusBlock) {
  Check ($specStatus.Rows.Count -eq $codeStatus.Count) `
    "status cells: the spec has $($specStatus.Rows.Count) row(s) but the code has $($codeStatus.Count)"
  $n = [Math]::Min($specStatus.Rows.Count, $codeStatus.Count)
  for ($i = 0; $i -lt $n; $i++) {
    $row = $specStatus.Rows[$i]
    $code = $codeStatus[$i]
    Check ((Strip-Ticks $row[0]) -eq $code.Key) `
      "status cell #$($i + 1): spec key $(Strip-Ticks $row[0]) vs code key $($code.Key) (Shell.tsx:$($code.No))"
    Check ($row[1] -eq $code.Label) `
      "status cell $(Strip-Ticks $row[0]): spec name '$($row[1])' vs code name '$($code.Label)' (Shell.tsx:$($code.No))"
    $wantResident = ($row[2] -eq 'yes')
    if ($row[2] -ne 'yes' -and $row[2] -ne 'no') {
      Fail "status cell $(Strip-Ticks $row[0]): the resident column must be yes/no, found '$($row[2])'"
    }
    Check ($wantResident -eq $code.Resident) `
      "status cell $(Strip-Ticks $row[0]): spec resident=$($row[2]) vs code resident=$($code.Resident) (Shell.tsx:$($code.No))"
  }
}

# --- code: PRIMARY ----------------------------------------------------------

$primaryBlock = Extract-Block $navLines 'export const PRIMARY: readonly PrimaryItem\[\] = \[' '^\];' 'nav.ts PRIMARY'
$primaries = @()
if ($null -ne $primaryBlock) {
  $cur = $null
  $mode = ''
  foreach ($l in $primaryBlock.Lines) {
    $t = $l.Text
    if ($t -match '^  \{\s*$') {
      $cur = @{ Key = ''; Label = ''; Sections = @(); Items = @(); No = $l.No }
      $primaries += , $cur
      $mode = ''
      continue
    }
    if ($null -eq $cur) {
      if ($t.Trim() -ne '') { Fail "nav.ts:$($l.No): a line before the first primary entry: $($t.Trim())" }
      continue
    }
    if ($t -match '^    key: "([^"]+)",\s*$') { $cur.Key = $Matches[1]; continue }
    if ($t -match '^    label: "([^"]+)",\s*$') { $cur.Label = $Matches[1]; continue }
    if ($t -match '^    secondary: null,\s*$') { $mode = 'none'; continue }
    if ($t -match '^    secondary: \{\s*$') { $mode = ''; continue }
    if ($t -match '^      sections: \[\s*$') { $mode = 'sections'; continue }
    if ($t -match '^      items: \[\s*$') { $mode = 'items'; continue }
    # a page with exactly one section writes it on one line; that shape is fine,
    # but an inline list this script cannot read is a failure, not something to skip
    if ($t -match '^      sections: \[.+\],\s*$') {
      $innerText = $Matches[0]
      $found = [regex]::Matches($innerText, '\{ key: "([^"]+)", label: "([^"]+)", source: "(\w+)", rootItem: (?:"([^"]+)"|null) \}')
      if ($found.Count -eq 0) {
        Fail "nav.ts:$($l.No): a compact sections list this script does not understand: $($t.Trim())"
      }
      foreach ($m in $found) {
        $rootItem = $null
        if ($m.Groups[4].Success) { $rootItem = $m.Groups[4].Value }
        $cur.Sections += , @{ Key = $m.Groups[1].Value; Label = $m.Groups[2].Value; Source = $m.Groups[3].Value; RootItem = $rootItem; No = $l.No }
      }
      continue
    }
    if ($t -match '^      items: \[.+\],\s*$') {
      $innerText = $Matches[0]
      $found = [regex]::Matches($innerText, '\{ section: "([^"]+)", key: "([^"]+)", label: "([^"]+)" \}')
      if ($found.Count -eq 0) {
        Fail "nav.ts:$($l.No): a compact items list this script does not understand: $($t.Trim())"
      }
      foreach ($m in $found) {
        $cur.Items += , @{ Section = $m.Groups[1].Value; Key = $m.Groups[2].Value; Label = $m.Groups[3].Value; No = $l.No }
      }
      continue
    }
    if ($t -match '^      \],\s*$') { $mode = ''; continue }
    if ($t -match '^    \},\s*$') { continue }
    if ($t -match '^  \},\s*$') { $mode = ''; continue }
    if ($t.Trim().StartsWith('//')) { continue }
    if ($t -match '^        \{ key: "([^"]+)", label: "([^"]+)", source: "(\w+)", rootItem: (?:"([^"]+)"|null) \},\s*$') {
      if ($mode -ne 'sections') {
        Fail "nav.ts:$($l.No): a section row outside a sections list: $($t.Trim())"
        continue
      }
      $rootItem = $null
      if ($Matches[4] -ne '') { $rootItem = $Matches[4] }
      $cur.Sections += , @{ Key = $Matches[1]; Label = $Matches[2]; Source = $Matches[3]; RootItem = $rootItem; No = $l.No }
      continue
    }
    if ($t -match '^        \{ section: "([^"]+)", key: "([^"]+)", label: "([^"]+)" \},\s*$') {
      if ($mode -ne 'items') {
        Fail "nav.ts:$($l.No): an item row outside an items list: $($t.Trim())"
        continue
      }
      $cur.Items += , @{ Section = $Matches[1]; Key = $Matches[2]; Label = $Matches[3]; No = $l.No }
      continue
    }
    if ($t.Trim() -ne '') {
      Fail "nav.ts:$($l.No): a line inside PRIMARY this script does not understand: $($t.Trim())"
    }
  }
  foreach ($p in $primaries) {
    if ($p.Key -eq '' -or $p.Label -eq '') {
      Fail "nav.ts:$($p.No): a primary entry without a key or a label"
    }
  }
}

# --- code: PrimaryKey union, PRODUCT_KEYS, TOOL_KEYS -------------------------

$declaredKeys = @()
for ($i = 0; $i -lt $navLines.Count; $i++) {
  if ($navLines[$i] -match '^export type PrimaryKey =') {
    $j = $i
    while ($j -lt $navLines.Count) {
      foreach ($m in [regex]::Matches($navLines[$j], '"(\w+)"')) { $declaredKeys += $m.Groups[1].Value }
      if ($navLines[$j] -match ';\s*$') { break }
      $j++
    }
    break
  }
}
Check ($declaredKeys.Count -gt 0) 'nav.ts: could not parse the PrimaryKey union'

function Read-KeyArray($lines, [string]$name, [string]$what) {
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match ("^export const " + $name + ": readonly PrimaryKey\[\] = \[(.+)\];\s*$")) {
      $keys = @()
      foreach ($m in [regex]::Matches($Matches[1], '"(\w+)"')) { $keys += $m.Groups[1].Value }
      return $keys
    }
  }
  Fail "$what : could not parse $name"
  return @()
}

$productKeys = Read-KeyArray $navLines 'PRODUCT_KEYS' 'nav.ts'
$toolKeys = Read-KeyArray $navLines 'TOOL_KEYS' 'nav.ts'

if ($primaries.Count -gt 0) {
  Check ($declaredKeys.Count -eq $primaries.Count) `
    "nav.ts: PrimaryKey declares $($declaredKeys.Count) member(s) but PRIMARY has $($primaries.Count) entry(ies)"
}

# --- code: hrefOf() in Shell.tsx --------------------------------------------

$hrefBlock = Extract-Block $shellLines '^function hrefOf\(k: PrimaryKey\): string \{' '^\}' 'Shell.tsx hrefOf'
$hrefs = @{}
if ($null -ne $hrefBlock) {
  for ($i = 0; $i -lt $hrefBlock.Lines.Count; $i++) {
    $l = $hrefBlock.Lines[$i]
    if ($l.Text -match '^\s*case "(\w+)":\s*$') {
      $key = $Matches[1]
      if ($i + 1 -ge $hrefBlock.Lines.Count) {
        Fail "Shell.tsx:$($l.No): case '$key' has no return on the next line"
        continue
      }
      $next = $hrefBlock.Lines[$i + 1]
      if ($next.Text -match '^\s*return "([^"]+)";\s*$') {
        $hrefs[$key] = $Matches[1]
        $i++
        continue
      }
      Fail "Shell.tsx:$($next.No): expected a return after case '$key', found: $($next.Text.Trim())"
    }
    elseif ($l.Text -match '^\s*case ') {
      Fail "Shell.tsx:$($l.No): a case this script does not understand: $($l.Text.Trim())"
    }
  }
}

foreach ($k in $declaredKeys) {
  if (-not $hrefs.ContainsKey($k)) { Fail "Shell.tsx: hrefOf() has no path for primary '$k'" }
}

# --- compare: primary nav ---------------------------------------------------

if ($null -ne $specNav) {
  Check ($specNav.Rows.Count -eq $primaries.Count) `
    "primary nav: the spec has $($specNav.Rows.Count) row(s) but nav.ts PRIMARY has $($primaries.Count)"
  $n = [Math]::Min($specNav.Rows.Count, $primaries.Count)
  for ($i = 0; $i -lt $n; $i++) {
    $row = $specNav.Rows[$i]
    $code = $primaries[$i]
    $key = Strip-Ticks $row[0]
    Check ($key -eq $code.Key) "primary nav #$($i + 1): spec key '$key' vs code key '$($code.Key)' (nav.ts:$($code.No))"
    Check ($row[1] -eq $code.Label) "primary nav '$key': spec name '$($row[1])' vs code name '$($code.Label)' (nav.ts:$($code.No))"
    $wantSection = $row[2]
    if ($wantSection -ne 'product' -and $wantSection -ne 'tool') {
      Fail "primary nav '$key': the section column must be product/tool, found '$wantSection'"
    }
    else {
      $inProduct = $productKeys -contains $key
      $inTool = $toolKeys -contains $key
      Check ($inProduct -ne $inTool) "nav.ts: '$key' must be in exactly one of PRODUCT_KEYS / TOOL_KEYS"
      $actual = if ($inProduct) { 'product' } else { 'tool' }
      Check ($wantSection -eq $actual) "primary nav '$key': spec section '$wantSection' vs code section '$actual'"
    }
    $wantPath = Strip-Ticks $row[3]
    if ($hrefs.ContainsKey($key)) {
      Check ($wantPath -eq $hrefs[$key]) "primary nav '$key': spec path '$wantPath' vs Shell.tsx hrefOf '$($hrefs[$key])'"
    }
  }
  Check (($productKeys.Count + $toolKeys.Count) -eq $declaredKeys.Count) `
    "nav.ts: PRODUCT_KEYS ($($productKeys.Count)) + TOOL_KEYS ($($toolKeys.Count)) != PrimaryKey members ($($declaredKeys.Count))"
}

# --- compare: secondary sections --------------------------------------------

$codeSections = @()
foreach ($p in $primaries) {
  foreach ($s in $p.Sections) { $codeSections += , @{ Primary = $p.Key; Key = $s.Key; Label = $s.Label; Source = $s.Source; RootItem = $s.RootItem; No = $s.No } }
}

if ($null -ne $specSections) {
  Check ($specSections.Rows.Count -eq $codeSections.Count) `
    "secondary sections: the spec has $($specSections.Rows.Count) row(s) but the code has $($codeSections.Count)"
  $n = [Math]::Min($specSections.Rows.Count, $codeSections.Count)
  for ($i = 0; $i -lt $n; $i++) {
    $row = $specSections.Rows[$i]
    $code = $codeSections[$i]
    $pKey = Strip-Ticks $row[0]
    $sKey = Strip-Ticks $row[1]
    Check ($pKey -eq $code.Primary) "section #$($i + 1): spec primary '$pKey' vs code primary '$($code.Primary)' (nav.ts:$($code.No))"
    Check ($sKey -eq $code.Key) "section #$($i + 1) of '$pKey': spec key '$sKey' vs code key '$($code.Key)' (nav.ts:$($code.No))"
    Check ($row[2] -eq $code.Label) "section '$pKey/$sKey': spec name '$($row[2])' vs code name '$($code.Label)' (nav.ts:$($code.No))"
    Check ((Strip-Ticks $row[3]) -eq $code.Source) "section '$pKey/$sKey': spec source '$(Strip-Ticks $row[3])' vs code source '$($code.Source)' (nav.ts:$($code.No))"
    if ($row.Count -lt 5) {
      Fail "section '$pKey/$sKey': the spec table has no root-item column (the code says '$($code.RootItem)')"
    }
    else {
      $codeRoot = 'none'
      if ($null -ne $code.RootItem) { $codeRoot = $code.RootItem }
      Check ((Strip-Ticks $row[4]) -eq $codeRoot) `
        "section '$pKey/$sKey': spec root item '$($row[4])' vs code '$codeRoot' (nav.ts:$($code.No))"
    }
  }
}

# a page with no secondary must have no rows, and the other way round
foreach ($p in $primaries) {
  $rows = @($codeSections | Where-Object { $_.Primary -eq $p.Key })
  if ($p.Sections.Count -eq 0) {
    Check ($rows.Count -eq 0) "nav.ts: '$($p.Key)' has no sections but the spec table lists $($rows.Count)"
  }
  $specRows = @()
  if ($null -ne $specSections) {
    $specRows = @($specSections.Rows | Where-Object { (Strip-Ticks $_[0]) -eq $p.Key })
  }
  Check ($specRows.Count -eq $p.Sections.Count) `
    "nav.ts: '$($p.Key)' has $($p.Sections.Count) section(s) but the spec table has $($specRows.Count) row(s)"
}

# --- compare: secondary items ----------------------------------------------

$codeItems = @()
foreach ($p in $primaries) {
  foreach ($it in $p.Items) { $codeItems += , @{ Primary = $p.Key; Section = $it.Section; Key = $it.Key; Label = $it.Label; No = $it.No } }
}

# every item must point at a section that exists on the same page
foreach ($it in $codeItems) {
  $page = $primaries | Where-Object { $_.Key -eq $it.Primary } | Select-Object -First 1
  $found = @($page.Sections | Where-Object { $_.Key -eq $it.Section })
  if ($found.Count -eq 0) {
    Fail "nav.ts:$($it.No): item '$($it.Key)' belongs to section '$($it.Section)', which '$($it.Primary)' does not have"
  }
}

if ($null -ne $specItems) {
  Check ($specItems.Rows.Count -eq $codeItems.Count) `
    "secondary items: the spec has $($specItems.Rows.Count) row(s) but the code has $($codeItems.Count)"
  $n = [Math]::Min($specItems.Rows.Count, $codeItems.Count)
  for ($i = 0; $i -lt $n; $i++) {
    $row = $specItems.Rows[$i]
    $code = $codeItems[$i]
    $pKey = Strip-Ticks $row[0]
    $sKey = Strip-Ticks $row[1]
    $iKey = Strip-Ticks $row[2]
    Check ($pKey -eq $code.Primary) "item #$($i + 1): spec primary '$pKey' vs code primary '$($code.Primary)' (nav.ts:$($code.No))"
    Check ($sKey -eq $code.Section) "item #$($i + 1) of '$pKey': spec section '$sKey' vs code section '$($code.Section)' (nav.ts:$($code.No))"
    Check ($iKey -eq $code.Key) "item #$($i + 1) of '$pKey': spec key '$iKey' vs code key '$($code.Key)' (nav.ts:$($code.No))"
    Check ($row[3] -eq $code.Label) "item '$pKey/$iKey': spec name '$($row[3])' vs code name '$($code.Label)' (nav.ts:$($code.No))"
  }
}

# --- check G: every static secondary item resolves to a route that exists -----
#
# `secondaryHref()` builds `<primary path>/<item key>` mechanically, and its
# return type is `string` -- not the router's literal union -- so nothing in the
# type system notices when a nav entry points at a path that was never
# registered. Four of them did, and clicking one showed "Not Found" in the real
# window. The rule this check enforces:
#
#   - an item may sit on the primary page itself  -> that section's `rootItem`
#   - every other static item needs its own route -> `page("<path>"` or
#     `path: "<path>"` in router.tsx

$routerLines = Read-TextLines (Join-Path $root 'web\src\routes\router.tsx')
$routePaths = New-Object System.Collections.Generic.List[string]
for ($i = 0; $i -lt $routerLines.Count; $i++) {
  foreach ($m in [regex]::Matches($routerLines[$i], 'page\("([^"]+)"')) { $routePaths.Add($m.Groups[1].Value) }
  foreach ($m in [regex]::Matches($routerLines[$i], 'path: "([^"]+)"')) { $routePaths.Add($m.Groups[1].Value) }
}
Check ($routePaths.Count -gt 0) 'router.tsx: no route paths were found -- this script is looking at the wrong shape'

$sectionByKey = @{}
foreach ($p in $primaries) {
  foreach ($s in $p.Sections) {
    $sectionByKey["$($p.Key)/$($s.Key)"] = $s
    if ($s.Source -ne 'static' -and $null -ne $s.RootItem) {
      Fail "nav.ts:$($s.No): section '$($p.Key)/$($s.Key)' has source '$($s.Source)' and a rootItem -- only a static section can put an item on the primary page"
    }
    if ($null -ne $s.RootItem) {
      $hit = @($p.Items | Where-Object { $_.Section -eq $s.Key -and $_.Key -eq $s.RootItem })
      Check ($hit.Count -eq 1) `
        "nav.ts:$($s.No): section '$($p.Key)/$($s.Key)' says its root item is '$($s.RootItem)', but no such item is in that section"
    }
  }
}

$routedItems = 0
foreach ($it in $codeItems) {
  $sectionKey = "$($it.Primary)/$($it.Section)"
  if (-not $sectionByKey.ContainsKey($sectionKey)) { continue }
  $section = $sectionByKey[$sectionKey]
  if ($section.Source -ne 'static') { continue }
  if (-not $hrefs.ContainsKey($it.Primary)) { continue }
  $routedItems++
  if ($section.RootItem -eq $it.Key) { continue }
  $base = $hrefs[$it.Primary]
  $sep = ''
  if ($base -ne '/') { $sep = '/' }
  $want = "$base$sep$($it.Key)"
  Check ($routePaths.Contains($want)) `
    "router.tsx: '$($it.Primary)/$($it.Key)' (nav.ts:$($it.No)) links to '$want', but no route with that path exists"
}

# --- compare: shell sizes vs Shell.css --------------------------------------

$cssText = ([System.IO.File]::ReadAllText($cssPath, [System.Text.Encoding]::UTF8))
$cssText = [regex]::Replace($cssText, '/\*.*?\*/', '', 'Singleline')
$cssDecls = @()
$stack = New-Object System.Collections.Generic.List[string]
foreach ($line in ($cssText -split "`n")) {
  $t = $line.Trim()
  if ($t -eq '') { continue }
  if ($t -eq '}') {
    if ($stack.Count -gt 0) { $stack.RemoveAt($stack.Count - 1) }
    continue
  }

  # a whole rule on one line: `.sel { --a: 1px; --b: 2px; }`
  $compact = [regex]::Match($t, '^(?<sel>[^{}]*?)\{(?<body>[^{}]*)\}\s*$')
  if ($compact.Success) {
    $sel = $compact.Groups['sel'].Value.Trim()
    if ($sel -eq '') { $sel = '@' }
    foreach ($d in [regex]::Matches($compact.Groups['body'].Value, '(?<name>--[\w-]+)\s*:\s*(?<value>[^;]+);')) {
      $cssDecls += , @{ Selector = $sel; Name = $d.Groups['name'].Value; Value = $d.Groups['value'].Value.Trim() }
    }
    continue
  }

  $decl = [regex]::Match($t, '^(?<name>--[\w-]+)\s*:\s*(?<value>[^;]+);\s*$')
  if ($decl.Success) {
    $chain = if ($stack.Count -gt 0) { ($stack -join ' > ') } else { '@' }
    $cssDecls += , @{ Selector = $chain; Name = $decl.Groups['name'].Value; Value = $decl.Groups['value'].Value.Trim() }
    continue
  }

  $open = [regex]::Match($t, '^(?<sel>.*)\{\s*$')
  if ($open.Success) {
    $sel = $open.Groups['sel'].Value.Trim()
    if ($sel -eq '') { $sel = '@' }
    $stack.Add($sel)
    continue
  }

  # Anything else is normally a selector continued on the next line -- but a line
  # that DECLARES a custom property and was not understood above must be a failure,
  # otherwise a variable can be added in a shape the size check never sees.
  if ($t -match '--[\w-]+\s*:') {
    Fail "Shell.css: a custom-property declaration this script does not understand: $t"
  }
}

$sizeNames = @()
if ($null -ne $specSizes) {
  foreach ($row in $specSizes.Rows) {
    $name = Strip-Ticks $row[0]
    if (-not ($sizeNames -contains $name)) { $sizeNames += $name }
  }
  foreach ($row in $specSizes.Rows) {
    $name = Strip-Ticks $row[0]
    $scope = $row[1]
    $want = $row[2]
    if ($scope -ne 'narrow' -and $scope -ne 'default') {
      Fail "shell sizes: the scope column must be narrow/default, found '$scope'"
      continue
    }
    $all = @($cssDecls | Where-Object { $_.Name -eq $name })
    $narrow = @($all | Where-Object { $_.Selector -match '\[data-rail="narrow"\]' })
    $default = @($all | Where-Object { $_.Selector -notmatch '\[data-rail="narrow"\]' })
    if ($scope -eq 'narrow') {
      Check ($narrow.Count -eq 1) "Shell.css: expected exactly one '$name' inside [data-rail=narrow], found $($narrow.Count)"
      foreach ($d in $narrow) {
        Check ($d.Value -eq $want) "'$name' in [data-rail=narrow]: spec '$want' vs Shell.css '$($d.Value)'"
      }
    }
    else {
      Check ($default.Count -eq 1) "Shell.css: expected exactly one '$name' outside [data-rail=narrow], found $($default.Count)"
      foreach ($d in $default) {
        Check ($d.Value -eq $want) "'$name' (default scope): spec '$want' vs Shell.css '$($d.Value)'"
      }
    }
  }
  # bidirectional: no declaration of these names may be left unexplained
  foreach ($name in $sizeNames) {
    $rows = @($specSizes.Rows | Where-Object { (Strip-Ticks $_[0]) -eq $name })
    $all = @($cssDecls | Where-Object { $_.Name -eq $name })
    Check ($all.Count -eq $rows.Count) `
      "Shell.css: '$name' is declared $($all.Count) time(s) but the spec table has $($rows.Count) row(s) for it"
  }
}

# --- version parity (code only) ---------------------------------------------

function Find-Version([string]$path, [string]$pattern, [string]$what) {
  $text = [System.IO.File]::ReadAllText($path, [System.Text.Encoding]::UTF8)
  $m = [regex]::Match($text, $pattern)
  if (-not $m.Success) {
    Fail "$what : could not find the version"
    return ''
  }
  return $m.Groups[1].Value
}

$vApp = Find-Version (Join-Path $root 'web\src\app\version.ts') 'APP_VERSION\s*=\s*"([^"]+)"' 'web/src/app/version.ts'
$vPkg = Find-Version (Join-Path $root 'package.json') '"version"\s*:\s*"([^"]+)"' 'package.json'
$vTauri = Find-Version (Join-Path $root 'src-tauri\tauri.conf.json') '"version"\s*:\s*"([^"]+)"' 'src-tauri/tauri.conf.json'
Check ($vApp -eq $vPkg) "version: version.ts '$vApp' vs package.json '$vPkg'"
Check ($vApp -eq $vTauri) "version: version.ts '$vApp' vs src-tauri/tauri.conf.json '$vTauri'"

# --- verdict -----------------------------------------------------------------

if ($script:Failures.Count -gt 0) {
  Write-Host "FAIL: $($script:Failures.Count) shell-contract mismatch(es)"
  foreach ($f in $script:Failures) { Write-Host "  - $f" }
  exit 1
}

Write-Host ("Shell contract: {0} status cell(s) x {1} nav item(s) x {2} section(s) x {3} sub-item(s) x {4} size(s) x {5} routed item(s) -- version {6}" -f `
    $specStatus.Rows.Count, $specNav.Rows.Count, $specSections.Rows.Count, $specItems.Rows.Count, $specSizes.Rows.Count, $routedItems, $vApp)
Write-Host 'OK: every cell of the shell spec tables matches the code, and every static sub-item has a route.'
exit 0
