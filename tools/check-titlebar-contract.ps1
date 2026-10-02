# =============================================================================
# check-titlebar-contract.ps1 -- the titlebar contract is written TWICE
#                                (a markdown table in the UI design spec and a
#                                TypeScript table in web/src/titlebar/contract.ts)
#                                and the two must agree cell by cell
#
#                                Since v2.3 this script covers TWO tables:
#                                  * the event contract   (spec 4.5.1 -> contract.ts)
#                                  * the four glyph names (spec 4.5.2 -> glyphs.tsx)
# =============================================================================
#
# WHY THIS EXISTS
#
#   `decorations: false` (src-tauri/tauri.conf.json) removes the system
#   titlebar, so every bit of titlebar behaviour is ours to re-implement. The
#   failure mode is always the same shape: ONE ELEMENT x ONE EVENT FAMILY is
#   forgotten, and nothing notices.
#
# THE DEFECTS THIS ENCODES
#
#   - faa7fa1: the three window controls excluded `pointerdown` but not
#     `dblclick`, so double-clicking CLOSE maximized the window and then closed
#     it. One cell of one table.
#   - The same shape once more, found by the table-driven test: a DISABLED
#     search box never receives React's synthetic `onDoubleClick`, so
#     "swallow it on that element" silently did nothing. One cell again.
#   - v2.3, the other half of the same component: the maximize button drew the
#     wrong PICTURE. TitleBar rendered `MorphGlyph` (the morphicons spike --
#     "menu" morphing into "close") in the maximize slot, so the three controls
#     read `- menu x` instead of the spec's `- square x`, and there was no
#     `square` anywhere. Nothing noticed: the glyph test asserted the SPIKE's
#     paths, and TitleBar.test never touched an svg. The USER's eye found it.
#
#   So both halves are now checked from both sides:
#     * web/src/titlebar/contract.test.tsx  -- event table -> real DOM behaviour
#     * web/src/titlebar/glyphs.test.tsx    -- glyph table -> the rendered `d`
#     * this script                          -- spec tables  -> code tables
#
# WHAT IT ASSERTS
#
#   1. The event family list is the same on both sides, in the same order.
#   2. The element id list is the same on both sides (8 rows today).
#   3. For every element and family, the spec symbol means the same cell as the
#      code: swallow / bubble / skip / na.
#   4. The status column agrees: implemented / structural / pending.
#   5. Every row on BOTH sides parsed. A row whose format drifted is a FAILURE,
#      not a silent skip -- otherwise this script would quietly check less and
#      less while still printing OK.
#   6. The four window-control GLYPH names listed under spec 4.5.2 are exactly
#      the names of WINDOW_GLYPH_NAMES in web/src/titlebar/glyphs.tsx, in the
#      same order -- and every declared name has a shape, because a name with no
#      paths renders an empty <svg>: perfectly invisible, and invisible is the
#      failure mode this file exists to kill.
#
# WHAT IT DELIBERATELY DOES NOT DO
#
#   - It does not check the DOM; that is contract.test.tsx's job.
#   - It does not check that the spec's prose is true or that the spec's cells
#     are the RIGHT choice -- only that the two tables say the same thing. A
#     wrong-but-consistent pair passes here, and only a human reading the
#     contract section can catch that. This script makes drift impossible, not
#     decisions correct.
#   - NO ALLOW-LIST: there is exactly one contract, so an exception would mean
#     the contract itself is wrong.
#
# ASCII-ONLY: this file must stay ASCII. Non-ASCII in a .ps1 breaks parsing
# under Windows PowerShell -- that mistake has been made in this repo more than
# once, including in this file's first draft, which read as mojibake and turned
# the spec path into a filename that does not exist. So both the spec's
# FILENAME and its SYMBOLS are built from code points below, and an
# unrecognised symbol is a FAILURE -- which is also what makes a wrong code
# point loud.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Walk up for .git rather than counting directory levels -- that mistake has
# been made in this repo (a script moved one level deeper silently scanned the
# wrong tree).
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $parent = Split-Path $dir -Parent
    if ($parent -eq $dir) { $dir = $null } else { $dir = $parent }
}
if (-not $dir) { throw 'check-titlebar-contract: could not find .git walking up from the script' }

# The design spec's filename is Chinese: "UI" + she-ji-gui-ge + ".md".
$specName = 'UI' +
    [char]0x8BBE + [char]0x8BA1 + [char]0x89C4 + [char]0x683C +
    '.md'
$specRel = 'docs\' + $specName
$specPath = Join-Path $dir $specRel
$tsRel = 'web\src\titlebar\contract.ts'
$tsPath = Join-Path $dir $tsRel
foreach ($p in @($specPath, $tsPath)) {
    if (-not (Test-Path $p)) { throw "check-titlebar-contract: missing $p" }
}

# --- the spec's symbols, built from code points (see ASCII-ONLY above) -------
$SYM_SWALLOW = [string][char]0x541E                                        # tun
$SYM_BUBBLE = [string][char]0x5192                                         # mao
$SYM_SKIP = [string][char]0x4E0D + [string][char]0x505A                    # bu-zuo
$SYM_NA = [string][char]0x4E0D + [string][char]0x9002 + [string][char]0x7528  # bu-shi-yong
$STA_DONE = [string][char]0x5DF2 + [string][char]0x5B9E + [string][char]0x73B0        # yi-shi-xian
$STA_STRUCTURAL = [string][char]0x7ED3 + [string][char]0x6784 + [string][char]0x6027  # jie-gou-xing
$STA_PENDING = [string][char]0x672A + [string][char]0x5B9E + [string][char]0x73B0    # wei-shi-xian

$symbolToCell = @{}
$symbolToCell[$SYM_SWALLOW] = 'swallow'
$symbolToCell[$SYM_BUBBLE] = 'bubble'
$symbolToCell[$SYM_SKIP] = 'skip'
$symbolToCell[$SYM_NA] = 'na'

$statusToCode = @{}
$statusToCode[$STA_DONE] = 'done'
$statusToCode[$STA_STRUCTURAL] = 'structural'
$statusToCode[$STA_PENDING] = 'pending'

function Convert-Symbol([string]$text, [string]$where) {
    $clean = $text.Trim()
    if ($symbolToCell.ContainsKey($clean)) { return $symbolToCell[$clean] }
    throw "check-titlebar-contract: unrecognised symbol '$clean' at $where -- the spec's symbol table and this script disagree"
}

function Convert-Status([string]$text, [string]$where) {
    $clean = $text.Trim()
    # The status cell may carry a parenthetical reason after the word, so a
    # prefix match is intended here.
    foreach ($key in $statusToCode.Keys) {
        if ($clean.StartsWith($key)) { return $statusToCode[$key] }
    }
    throw "check-titlebar-contract: unrecognised status '$clean' at $where"
}

# --- the spec side -----------------------------------------------------------
$specLines = [System.IO.File]::ReadAllLines($specPath, [System.Text.Encoding]::UTF8)
$headAt = -1
for ($i = 0; $i -lt $specLines.Count; $i++) {
    if ($specLines[$i] -match '^####\s+4\.5\.1\s') { $headAt = $i; break }
}
if ($headAt -lt 0) { throw "check-titlebar-contract: no '#### 4.5.1' heading in $specRel" }

$tblAt = -1
$scanEnd = [Math]::Min($headAt + 80, $specLines.Count - 1)
for ($i = $headAt; $i -le $scanEnd; $i++) {
    if ($specLines[$i] -match '^\|\s*#\s*\|.*`pointerdown`') { $tblAt = $i; break }
}
if ($tblAt -lt 0) { throw "check-titlebar-contract: no contract table header under 4.5.1 in $specRel" }

$specFamilies = @([regex]::Matches($specLines[$tblAt], '`([a-z]+)`') | ForEach-Object { $_.Groups[1].Value })
if ($specFamilies.Count -lt 2) { throw 'check-titlebar-contract: the table header names fewer than two event families' }

$specRows = New-Object 'System.Collections.Generic.List[object]'
$failures = New-Object 'System.Collections.Generic.List[string]'
for ($i = $tblAt + 1; $i -lt $specLines.Count; $i++) {
    $line = $specLines[$i]
    if ($line -notmatch '^\|') { break }
    if ($line -match '^\|\s*-') { continue }
    $cells = @($line.Trim('|').Split('|') | ForEach-Object { $_.Trim() })
    $rowNo = $i + 1
    $where = "$specRel`:$rowNo"
    if ($cells.Count -ne ($specFamilies.Count + 3)) {
        $failures.Add("$where has $($cells.Count) cells, expected $($specFamilies.Count + 3)") | Out-Null
        continue
    }
    $idMatch = [regex]::Match($cells[1], '`([a-z]+)`')
    if (-not $idMatch.Success) {
        $failures.Add("$where has no backticked element id") | Out-Null
        continue
    }
    $specRows.Add([pscustomobject]@{
            id     = $idMatch.Groups[1].Value
            line   = $rowNo
            cells  = @($cells[2..(1 + $specFamilies.Count)] | ForEach-Object { Convert-Symbol $_ $where })
            status = (Convert-Status $cells[2 + $specFamilies.Count] $where)
        }) | Out-Null
}

# --- the code side -----------------------------------------------------------
$ts = [System.IO.File]::ReadAllText($tsPath, [System.Text.Encoding]::UTF8)

$famBlock = [regex]::Match($ts, 'EVENT_FAMILIES\s*=\s*\[(.*?)\]\s*as const')
if (-not $famBlock.Success) { throw "check-titlebar-contract: cannot find EVENT_FAMILIES in $tsRel" }
$codeFamilies = @([regex]::Matches($famBlock.Groups[1].Value, '"([a-z]+)"') | ForEach-Object { $_.Groups[1].Value })

$itemBlock = [regex]::Match($ts, 'TITLEBAR_ITEMS\s*=\s*\[(.*?)\]\s*as const satisfies', 'Singleline')
if (-not $itemBlock.Success) { throw "check-titlebar-contract: cannot find the TITLEBAR_ITEMS block in $tsRel" }
$body = $itemBlock.Groups[1].Value
$declaredRows = ([regex]::Matches($body, '\{\s*id:')).Count

$cellPatterns = @($codeFamilies | ForEach-Object { $_ + ': "([a-z]+)"' })
$rowPattern = '\{\s*id:\s*"([a-z]+)",\s*label:\s*"[^"]*",\s*status:\s*"([a-z]+)",\s*cells:\s*\{\s*' +
    ($cellPatterns -join ',\s*') + '\s*\}\s*\}'
$codeMatches = @([regex]::Matches($body, $rowPattern))

$codeRows = New-Object 'System.Collections.Generic.List[object]'
foreach ($m in $codeMatches) {
    $cells = @()
    for ($k = 0; $k -lt $codeFamilies.Count; $k++) { $cells += $m.Groups[3 + $k].Value }
    $codeRows.Add([pscustomobject]@{
            id     = $m.Groups[1].Value
            status = $m.Groups[2].Value
            cells  = $cells
        }) | Out-Null
}

# --- compare -----------------------------------------------------------------
if ($codeFamilies.Count -ne $specFamilies.Count) {
    $failures.Add("event family count differs: spec table has $($specFamilies.Count), ${tsRel} has $($codeFamilies.Count)") | Out-Null
} else {
    for ($k = 0; $k -lt $specFamilies.Count; $k++) {
        if ($specFamilies[$k] -ne $codeFamilies[$k]) {
            $failures.Add("event family #$($k + 1) differs: spec says '$($specFamilies[$k])', ${tsRel} says '$($codeFamilies[$k])'") | Out-Null
        }
    }
}

if ($declaredRows -ne $codeRows.Count) {
    $failures.Add("$tsRel declares $declaredRows row(s) but only $($codeRows.Count) parsed -- a row's format drifted, so this script would otherwise check less") | Out-Null
}

$specById = @{}
foreach ($r in $specRows) {
    if ($specById.ContainsKey($r.id)) { $failures.Add("$specRel lists the element '$($r.id)' twice") | Out-Null }
    $specById[$r.id] = $r
}
$codeById = @{}
foreach ($r in $codeRows) {
    if ($codeById.ContainsKey($r.id)) { $failures.Add("$tsRel lists the element '$($r.id)' twice") | Out-Null }
    $codeById[$r.id] = $r
}

foreach ($id in @(($specById.Keys + $codeById.Keys) | Sort-Object -Unique)) {
    if (-not $codeById.ContainsKey($id)) {
        $failures.Add("element '$id' is in the spec table (line $($specById[$id].line)) but not in $tsRel") | Out-Null
        continue
    }
    if (-not $specById.ContainsKey($id)) {
        $failures.Add("element '$id' is in $tsRel but not in the spec table") | Out-Null
        continue
    }
    $s = $specById[$id]
    $c = $codeById[$id]
    for ($k = 0; $k -lt $specFamilies.Count; $k++) {
        if ($s.cells[$k] -ne $c.cells[$k]) {
            $failures.Add("element '$id' / $($specFamilies[$k]): spec says $($s.cells[$k]), ${tsRel} says $($c.cells[$k])") | Out-Null
        }
    }
    if ($s.status -ne $c.status) {
        $failures.Add("element '$id' / status: spec says $($s.status), ${tsRel} says $($c.status)") | Out-Null
    }
}

# --- check 2: the four window-control glyph names (see the header) -----------
#
# v2.3: the maximize button drew the WRONG PICTURE -- it rendered the morphicons
# spike ("menu" morphing into "close") in that slot, so the spec's square was
# never drawn at all. The glyph names are now a second two-copy contract:
# this script checks spec 4.5.2 -> glyphs.tsx, and glyphs.test.tsx checks
# glyphs.tsx -> the `d` attributes that actually reach the DOM.
$glyphHeadAt = -1
for ($i = 0; $i -lt $specLines.Count; $i++) {
    if ($specLines[$i] -match '^####\s+4\.5\.2\s') { $glyphHeadAt = $i; break }
}
if ($glyphHeadAt -lt 0) { throw "check-titlebar-contract: no '#### 4.5.2' heading in $specRel" }

$specGlyphs = New-Object 'System.Collections.Generic.List[string]'
for ($i = $glyphHeadAt + 1; $i -lt $specLines.Count; $i++) {
    if ($specLines[$i] -match '^####\s') { break }
    $m = [regex]::Match($specLines[$i], '^\|\s*`([a-z]+)`\s*\|')
    if ($m.Success) { $specGlyphs.Add($m.Groups[1].Value) | Out-Null }
}
if ($specGlyphs.Count -eq 0) {
    $failures.Add("$specRel 4.5.2 has no table row whose first cell is a backticked glyph name") | Out-Null
}

$glyphTsRel = 'web\src\titlebar\glyphs.tsx'
$glyphTsPath = Join-Path $dir $glyphTsRel
if (-not (Test-Path $glyphTsPath)) { throw "check-titlebar-contract: missing $glyphTsPath" }
$glyphTs = [System.IO.File]::ReadAllText($glyphTsPath, [System.Text.Encoding]::UTF8)

$namesBlock = [regex]::Match($glyphTs, 'WINDOW_GLYPH_NAMES\s*=\s*\[(.*?)\]\s*as const', 'Singleline')
if (-not $namesBlock.Success) { throw "check-titlebar-contract: cannot find WINDOW_GLYPH_NAMES in $glyphTsRel" }
$codeGlyphs = @([regex]::Matches($namesBlock.Groups[1].Value, '"([a-z]+)"') | ForEach-Object { $_.Groups[1].Value })
if ($codeGlyphs.Count -eq 0) { throw "check-titlebar-contract: WINDOW_GLYPH_NAMES in $glyphTsRel names nothing" }

if ($specGlyphs.Count -ne $codeGlyphs.Count) {
    $failures.Add("glyph names: 4.5.2 lists $($specGlyphs.Count) ($($specGlyphs -join ', ')), ${glyphTsRel} declares $($codeGlyphs.Count) ($($codeGlyphs -join ', '))") | Out-Null
}
else {
    for ($k = 0; $k -lt $specGlyphs.Count; $k++) {
        if ($specGlyphs[$k] -ne $codeGlyphs[$k]) {
            $failures.Add("glyph #$($k + 1): 4.5.2 says '$($specGlyphs[$k])', ${glyphTsRel} says '$($codeGlyphs[$k])'") | Out-Null
        }
    }
}

# A declared name with no shape renders an EMPTY <svg>: invisible, and
# "invisible to the checks" is the exact failure mode this file exists to kill.
foreach ($g in $codeGlyphs) {
    if ($glyphTs -notmatch ('(?m)^\s*' + $g + ':\s*\[')) {
        $failures.Add("${glyphTsRel} declares the glyph '$g' but WINDOW_GLYPH_PATHS has no '${g}: [' entry -- it would draw nothing") | Out-Null
    }
}

$checked = $specRows.Count * $specFamilies.Count

Write-Host ''
Write-Host ("Titlebar contract: {0} element(s) x {1} event family(ies) = {2} cell(s)" -f $specRows.Count, $specFamilies.Count, $checked)
Write-Host ("Window glyphs:     {0} name(s): {1}" -f $codeGlyphs.Count, ($codeGlyphs -join ', '))

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host 'FAIL  the spec table and the code table disagree:'
    Write-Host ''
    foreach ($f in $failures) { Write-Host "      $f" }
    Write-Host ''
    Write-Host '      The contract has one authority and two copies. Fix whichever side is'
    Write-Host '      wrong -- usually the code, because that is what the user clicks -- and'
    Write-Host '      then make both copies say the same thing.'
    Write-Host ''
    Write-Host 'See the header of this script for the two defects it encodes.'
    exit 1
}

Write-Host 'OK: every cell of the spec table matches the code table, and the window glyph names agree.'
exit 0
