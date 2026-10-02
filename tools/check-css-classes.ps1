# =============================================================================
# check-css-classes.ps1 -- one class name, one owner; and every class used in
#                          a TSX className must be defined somewhere
# =============================================================================
#
# WHY THIS EXISTS -- two real defects, and a human eye found only the first one
#
# 1) THE COLLISION. 2026-10-02, the first time the desktop window was opened.
#
#    `web/src/styles.css` (the U0 self-check page) defined `.shell` for its own
#    wrapper. `web/src/routes/Shell.css` (the real application shell) defines
#    `.shell` too. Both files land in the same CSS bundle, so the winner is
#    decided by BUNDLE ORDER -- and `main.tsx` imports `styles.css` last. The
#    result: the whole desktop shell was laid out by the self-check page's
#    "max-width: 720px; margin: 0 auto; display: flex; flex-direction: column".
#
#    The titlebar (with minimize/maximize/close) sat in a 720px centred column
#    instead of the window's top-right corner, and the rail was stacked ABOVE
#    the content instead of beside it. Nothing threw: React rendered, all 281
#    front-end tests passed, the typechecker was happy.
#
#    What found it: a human eye, looking at a real window. Not jsdom (it has no
#    layout engine), not check-css-tokens (it only reads var(--token)), not
#    impeccable (anti-patterns only). A class name is a GLOBAL namespace, and a
#    collision in it is SILENT -- the loser does not warn, it just does nothing.
#
# 2) THE CLASS THAT NEVER EXISTED. Earlier in the project a page shipped
#    `className="page__error"` while the stylesheet only ever defined
#    `page__note`. Nothing failed, because an element carrying an unknown class
#    is simply an unstyled element. Same failure shape as the undefined CSS
#    token (see check-css-tokens.ps1): SILENT.
#
# WHAT IT ASSERTS
#
#   R1  a class name is defined by exactly ONE .css file under web/src
#       (two owners = bundle order decides, and nobody reads bundle order)
#   R2  every class name that appears in a `className` expression in
#       web/src/**/*.ts(x) is defined by some .css file
#
# WHAT IT DELIBERATELY DOES NOT DO
#
#   - NO ALLOW-LIST. Same reasoning as check-css-tokens.ps1: an allow-list is
#     how a check like this dies. In both cases above the fix is real work --
#     own the name (rename or delete the duplicate), or drop the dead hook.
#   - It compares NAMES. It does not resolve the cascade, specificity, or
#     computed values.
#   - It does NOT flag "defined but never used" CSS. Deciding that needs
#     selector composition (`:hover`, `:not()`, descendant combinators) and the
#     false positives would teach people to ignore this script.
#   - It does not see class names built from data at runtime (none exist
#     today). A token that ends in `-` right before a `${...}` in a template
#     literal is treated as a PREFIX requirement instead -- `btn--${tone}`
#     requires that some defined class starts with `btn--`.
#   - A class name a TEST looks up (`querySelector(".x")`) is a NOTE, not a
#     failure: deleting it would turn a passing test red. It is still printed,
#     because a class that only a test uses is a suspicious hook.
#   - Comments are stripped before looking for code -- the same lesson as
#     check-css-tokens.ps1 (a comment ABOUT a token is not a use of it) and
#     check-tauri-baseline.ps1 (a comment saying "this deliberately has no
#     plugin-fs" was read as a dependency). For TS the `//` strip is a
#     heuristic (`//` preceded by whitespace), which can cut the tail off a
#     string containing a URL; for class-name extraction that is acceptable.
#   - Line numbers point at the START of the className expression. A very long
#     expression spanning lines is reported at its first line.
#
# ASCII-ONLY: this file must stay ASCII. Non-ASCII in a .ps1 breaks parsing
# under Windows PowerShell (that mistake has been made in this repo before).

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
if (-not $dir) { throw 'check-css-classes: could not find .git walking up from the script' }

$srcDir = Join-Path $dir 'web\src'
if (-not (Test-Path $srcDir)) { throw "check-css-classes: no web/src at $srcDir" }
$srcPrefixLen = $srcDir.Length + 1

# --- text helpers ------------------------------------------------------------

function Add-LineStarts([System.Collections.Generic.List[int]]$list, [string]$text) {
    $list.Add(0)
    for ($i = 0; $i -lt $text.Length; $i++) {
        if ($text[$i] -eq "`n") { $list.Add($i + 1) }
    }
}

function Get-LineOf([System.Collections.Generic.List[int]]$map, [int]$index) {
    $hit = $map.BinarySearch($index)
    if ($hit -lt 0) { $hit = (-$hit) - 2 }
    if ($hit -lt 0) { $hit = 0 }
    return $hit + 1
}

function Remove-BlockComments([string]$text) {
    # Keep the line count identical so line numbers in the report still point
    # at the right place (see check-css-tokens.ps1).
    return [regex]::Replace($text, '/\*.*?\*/', {
            param($m)
            ($m.Value -split "`n" | ForEach-Object { '' }) -join "`n"
        }, 'Singleline')
}

function Remove-JsLineComments([string]$text) {
    return [regex]::Replace($text, '(?<=\s)//[^\n]*', '')
}

function Get-QuoteEnd([string]$text, [int]$start) {
    $q = $text[$start]
    $i = $start + 1
    while ($i -lt $text.Length) {
        $c = $text[$i]
        if ($c -eq '\') { $i += 2; continue }
        if ($c -eq $q) { return $i }
        $i++
    }
    return $text.Length - 1
}

function Get-BraceEnd([string]$text, [int]$start) {
    $depth = 0
    $i = $start
    while ($i -lt $text.Length) {
        $c = $text[$i]
        if ($c -eq '"' -or $c -eq "'" -or $c -eq '`') {
            # A string (or template) literal is opaque: braces inside it are
            # text, including the `${...}` of a template.
            $i = (Get-QuoteEnd $text $i) + 1
            continue
        }
        if ($c -eq '{') { $depth++ }
        elseif ($c -eq '}') {
            $depth--
            if ($depth -eq 0) { return $i }
        }
        $i++
    }
    return -1
}

# =============================================================================
# PASS 1 -- every class name defined by a CSS file
# =============================================================================

$cssFiles = @(Get-ChildItem $srcDir -Recurse -File -Filter '*.css' | Sort-Object FullName)

# class name -> list of "file:line" (one entry per definition site)
$defs = @{}
$definitionCount = 0

foreach ($f in $cssFiles) {
    $rel = $f.FullName.Substring($srcPrefixLen)
    $text = Remove-BlockComments (Get-Content $f.FullName -Raw -Encoding UTF8)
    $map = New-Object 'System.Collections.Generic.List[int]'
    Add-LineStarts $map $text

    # Walk the sheet: a run of text ending in `{` is a rule's prelude (or an
    # at-rule's prelude, which starts with `@` and defines no classes).
    $preludeStart = 0
    $depth = 0
    $i = 0
    while ($i -lt $text.Length) {
        $c = $text[$i]
        if ($c -eq '{') {
            $prelude = $text.Substring($preludeStart, $i - $preludeStart).Trim()
            if ($prelude.Length -gt 0 -and -not $prelude.StartsWith('@')) {
                # Classes inside a functional pseudo (`:not(.x)`, `:has(.x)`)
                # are REFERENCES, not definitions -- so drop parenthesised
                # groups before extracting.
                $noParens = ''
                $j = 0
                while ($j -lt $prelude.Length) {
                    if ($prelude[$j] -eq '(') {
                        $d = 0
                        while ($j -lt $prelude.Length) {
                            if ($prelude[$j] -eq '(') { $d++ }
                            elseif ($prelude[$j] -eq ')') { $d--; if ($d -eq 0) { break } }
                            $j++
                        }
                        $j++
                    } else {
                        $noParens += $prelude[$j]
                        $j++
                    }
                }
                # Point at the rule itself, not at the `}` of the rule above:
                # skip the leading whitespace of the prelude.
                $lead = $prelude.Length - $prelude.TrimStart().Length
                $line = Get-LineOf $map ($preludeStart + $lead)
                foreach ($cm in [regex]::Matches($noParens, '\.([A-Za-z_][A-Za-z0-9_-]*)')) {
                    $name = $cm.Groups[1].Value
                    if (-not $defs.ContainsKey($name)) { $defs[$name] = New-Object 'System.Collections.Generic.List[string]' }
                    $where = "$rel`:$line"
                    if (-not $defs[$name].Contains($where)) { $defs[$name].Add($where) }
                    $definitionCount++
                }
            }
            $depth++
            $preludeStart = $i + 1
        } elseif ($c -eq '}') {
            $depth--
            if ($depth -lt 0) { $depth = 0 }
            $preludeStart = $i + 1
        } elseif ($c -eq ';' -and $depth -eq 0) {
            $preludeStart = $i + 1
        }
        $i++
    }
}

# --- R1: exactly one owner per class name -----------------------------------

$collisions = @()
foreach ($name in ($defs.Keys | Sort-Object)) {
    $owners = @($defs[$name] | ForEach-Object { ($_ -split ':')[0] } | Sort-Object -Unique)
    if ($owners.Count -gt 1) {
        $collisions += [pscustomobject]@{ name = $name; where = @($defs[$name]) }
    }
}

# =============================================================================
# PASS 2 -- every class name used in a className expression
# =============================================================================

$codeFiles = @(Get-ChildItem $srcDir -Recurse -File |
    Where-Object { $_.Name -match '\.(tsx|ts)$' } | Sort-Object FullName)

# Class names that a test file mentions anywhere. Used only to soften a
# finding into a NOTE -- see "WHAT IT DELIBERATELY DOES NOT DO".
$testText = ''
foreach ($f in $codeFiles) {
    if ($f.Name -match '\.test\.tsx?$') { $testText += (Get-Content $f.FullName -Raw -Encoding UTF8) }
}

$unknown = @()   # @{ where = "file:line"; name = ...; prefix = bool }
$testOnly = @()  # names found only in tests

foreach ($f in $codeFiles) {
    $rel = $f.FullName.Substring($srcPrefixLen)
    $raw = Get-Content $f.FullName -Raw -Encoding UTF8
    $text = Remove-JsLineComments (Remove-BlockComments $raw)
    $map = New-Object 'System.Collections.Generic.List[int]'
    Add-LineStarts $map $text

    foreach ($m in [regex]::Matches($text, 'className\s*[=:]\s*')) {
        $valStart = $m.Index + $m.Length
        if ($valStart -ge $text.Length) { continue }
        $expr = $null
        $exprLine = Get-LineOf $map $valStart
        # A plain `className="a b"` IS the literal; the quotes were stripped
        # above, so it must not be scanned again for quotes (the first version
        # of this script did exactly that and therefore missed every plain
        # className -- it only ever reported names inside `{...}`).
        $plainLiteral = $false
        if ($text[$valStart] -eq '{') {
            $end = Get-BraceEnd $text $valStart
            if ($end -lt 0) { continue }
            $expr = $text.Substring($valStart + 1, $end - $valStart - 1)
        } elseif ($text[$valStart] -eq '"' -or $text[$valStart] -eq "'") {
            $end = Get-QuoteEnd $text $valStart
            $expr = $text.Substring($valStart + 1, $end - $valStart - 1)
            $plainLiteral = $true
        } else {
            continue
        }

        # Collect the class tokens of this expression.
        #
        # Only STRING LITERALS carry class names -- a bare identifier is a
        # variable, not a name. And a literal that sits on the right of a
        # comparison is a VALUE being compared (`state !== "default"`), not a
        # class name; that distinction is what keeps `cls("btn", state !==
        # "default" && \`btn--${state}\`)` from demanding a class called
        # `default`.
        $literals = New-Object 'System.Collections.Generic.List[object]'
        if ($plainLiteral) {
            $literals.Add([pscustomobject]@{ raw = $expr; template = $false; compare = $false })
        }
        $k = 0
        while (-not $plainLiteral -and $k -lt $expr.Length) {
            $c = $expr[$k]
            if ($c -ne '"' -and $c -ne "'" -and $c -ne '`') { $k++; continue }
            $end = Get-QuoteEnd $expr $k
            $inner = $expr.Substring($k + 1, $end - $k - 1)
            # Look back: a comparison operator immediately before the literal
            # means this is a value, not a class name.
            $before = $expr.Substring(0, $k).TrimEnd()
            $isCompareValue = $before.EndsWith('!==') -or $before.EndsWith('===') -or
            $before.EndsWith('!=') -or $before.EndsWith('==')
            $literals.Add([pscustomobject]@{ raw = $inner; template = ($c -eq '`'); compare = $isCompareValue })
            $k = $end + 1
        }

        foreach ($lit in $literals) {
            if ($lit.compare) { continue }
            $raw = $lit.raw
            $parts = @()
            if ($lit.template) {
                # Split a template literal into static text and `${...}`
                # expressions. Only string literals inside a `${...}` are
                # harvested, so `state` in `btn--${state}` is not mistaken for
                # a class name -- while `" shell__railItem--on"` inside a
                # ternary still is.
                $buf = ''
                $j = 0
                while ($j -lt $raw.Length) {
                    if ($raw[$j] -eq '$' -and ($j + 1) -lt $raw.Length -and $raw[$j + 1] -eq '{') {
                        $parts += [pscustomobject]@{ text = $buf; boundary = $true }
                        $buf = ''
                        $d = 0
                        $s = $j + 1
                        while ($s -lt $raw.Length) {
                            if ($raw[$s] -eq '{') { $d++ }
                            elseif ($raw[$s] -eq '}') { $d--; if ($d -eq 0) { break } }
                            $s++
                        }
                        $innerExpr = $raw.Substring($j + 2, $s - $j - 2)
                        foreach ($im in [regex]::Matches($innerExpr, '"([^"]*)"|''([^'']*)''')) {
                            $parts += [pscustomobject]@{ text = $im.Groups[1].Value + $im.Groups[2].Value; boundary = $false }
                        }
                        $j = $s + 1
                    } else {
                        $buf += $raw[$j]
                        $j++
                    }
                }
                $parts += [pscustomobject]@{ text = $buf; boundary = $false }
            } else {
                $parts += [pscustomobject]@{ text = $raw; boundary = $false }
            }

            foreach ($part in $parts) {
                $tokens = @([regex]::Matches($part.text, '[A-Za-z][A-Za-z0-9_-]*'))
                for ($t = 0; $t -lt $tokens.Count; $t++) {
                    $name = $tokens[$t].Value
                    $isPrefix = $false
                    if ($t -eq ($tokens.Count - 1) -and $part.boundary) {
                        # Right before a `${...}`: the name continues, so this
                        # is a prefix requirement (`btn--` -> `btn--primary`).
                        $isPrefix = $true
                    }
                    if ($name.Length -lt 2) { continue }
                    if ($defs.ContainsKey($name)) { continue }
                    if ($isPrefix) {
                        $matched = $false
                        foreach ($d in $defs.Keys) { if ($d.StartsWith($name)) { $matched = $true; break } }
                        if ($matched) { continue }
                    }
                    if ($testText.Contains($name)) {
                        $testOnly += $name
                        continue
                    }
                    $unknown += [pscustomobject]@{ where = "$rel`:$exprLine"; name = $name }
                }
            }
        }
    }
}

# --- report ------------------------------------------------------------------

Write-Output ''
Write-Output ("CSS classes: {0} file(s), {1} definition(s), {2} distinct name(s)" -f `
        $cssFiles.Count, $definitionCount, $defs.Keys.Count)

if ($testOnly.Count -gt 0) {
    Write-Output ''
    Write-Output ("NOTE  {0} class name(s) are used but defined nowhere -- EXCEPT in a test," -f (@($testOnly | Sort-Object -Unique).Count))
    Write-Output '      which is why they are not failures (deleting them would turn a'
    Write-Output '      passing test red). A class only a test uses is a suspicious hook:'
    foreach ($x in (@($testOnly | Sort-Object -Unique) | Sort-Object)) { Write-Output ("      " + $x) }
}

$bad = $false

if ($collisions.Count -gt 0) {
    $bad = $true
    Write-Output ''
    Write-Output ("FAIL  {0} class name(s) are defined by MORE THAN ONE css file:" -f $collisions.Count)
    Write-Output '      The last one in the bundle wins, so which rule is live depends on'
    Write-Output '      the import order in main.tsx -- not on anything anybody reads.'
    Write-Output '      Give the name ONE owner (rename one side, or delete the copy).'
    foreach ($c in $collisions) {
        Write-Output ("      .{0}" -f $c.name)
        foreach ($w in $c.where) { Write-Output ("          {0}" -f $w) }
    }
}

if ($unknown.Count -gt 0) {
    $bad = $true
    Write-Output ''
    Write-Output ("FAIL  {0} class name(s) are used in a className but defined NOWHERE:" -f $unknown.Count)
    Write-Output '      An element carrying an unknown class is simply unstyled -- there is'
    Write-Output '      no warning. Either the name is a typo, or the rule was never'
    Write-Output '      written; both are silent in the running app.'
    foreach ($u in ($unknown | Sort-Object -Property name)) {
        Write-Output ("      {0}  {1}" -f $u.where, $u.name)
    }
}

if ($bad) {
    Write-Output ''
    Write-Output 'See the header of this script for the two real defects it encodes'
    Write-Output '(a `.shell` defined by two files, and a `page__error` that never existed).'
    exit 1
}

Write-Output ''
Write-Output 'OK: every class name has exactly one owner, and every className resolves.'
exit 0
