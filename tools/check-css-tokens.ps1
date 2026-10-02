# =============================================================================
# check-css-tokens.ps1 -- 每一个 var(--token) 都必须有定义
# =============================================================================
#
# WHY THIS EXISTS
#
# A real defect in this repo, found while writing TitleBar.css:
#
#     --font-weight-medium      used in 13 places, defined in 0
#     --font-weight-semibold    used in 13 places, defined in 0
#
# The symptom is SILENT. `font-weight: var(--undefined)` makes the whole
# declaration invalid, so the weight falls back to `normal`. And on 13px
# Chinese text, `normal` and `medium` are almost indistinguishable -- so
# nobody reports it and no test goes red.
#
# The only reason it was found is that I manually checked the tokens I had
# just used. That is not a method -- it is luck. So it becomes a check.
#
# WHAT IT ASSERTS
#
#   every `var(--x)` that appears in web/src/**/*.css
#   is defined by some `--x:` in web/src/**/*.css
#
# WHAT IT DELIBERATELY DOES NOT DO
#
#   - It does NOT check CSS Modules class names. Those are covered by the
#     fact that a missing class is visible (unstyled element), and by the
#     component tests.
#   - It does NOT try to resolve computed values. Only existence.
#   - It does NOT allow an allow-list. An undefined token is a defect, and
#     an allow-list is how a check like this dies.
#
# ASCII-ONLY: this file must stay ASCII. Non-ASCII text in a .ps1 breaks
# parsing under Windows PowerShell (that mistake has been made here before).

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Walk up for .git rather than counting directory levels -- that mistake has
# been made in this repo (a script moved one level deeper silently scanned
# the wrong tree).
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $dir = Split-Path $dir -Parent
}
if (-not $dir) { throw 'check-css-tokens: could not find .git walking up from the script' }

$srcDir = Join-Path $dir 'web\src'
if (-not (Test-Path $srcDir)) { throw "check-css-tokens: no web/src at $srcDir" }

$files = @(Get-ChildItem $srcDir -Recurse -File -Filter '*.css')

# --- pass 1: every definition ------------------------------------------------
# `--name:` at the start of a declaration. The colon is what makes it a
# definition rather than a use -- `var(--x)` has no colon after the name.
$defined = @{}
foreach ($f in $files) {
    foreach ($m in [regex]::Matches((Get-Content $f.FullName -Raw -Encoding UTF8), '(?m)^\s*(--[a-zA-Z0-9-]+)\s*:')) {
        $defined[$m.Groups[1].Value] = $true
    }
}

# --- pass 2: every use -------------------------------------------------------
# `var(--name` -- with the optional fallback form `var(--name, fallback)`.
#
# NOTE ON FALLBACKS: `var(--maybe, 4px)` is NOT a use that needs a definition,
# because the fallback is exactly what makes it optional. But skipping those
# would create a hole big enough to hide the very defect this catches. So they
# are reported as a SEPARATE, softer category -- see the summary.
$missing = New-Object System.Collections.Generic.List[string]
$withFallback = New-Object System.Collections.Generic.List[string]

foreach ($f in $files) {
    $text = Get-Content $f.FullName -Raw -Encoding UTF8

    # ---------------------------------------------------------------------
    # Strip comments FIRST.
    #
    # This is not tidiness -- it is the difference between a real finding and
    # a false one. `styles.css` line 5 is prose:
    #
    #      *  1. **颜色一律 `var(--token)`**，不写字面值（ESLint 会拦）。
    #
    # That is a sentence ABOUT tokens, not a use of one. The first version of
    # this script reported `styles.css:5  --token` and would have sent someone
    # looking for a typo that does not exist.
    #
    # The SAME fix was needed in check-tauri-baseline.ps1, where the comment
    # saying "this deliberately has no tauri-plugin-fs" was matched as if it
    # were a dependency. The rule is the same in both places:
    #
    #     a check about CODE must look at CODE, not at PROSE.
    #
    # Block comments are replaced with a newline per line so that line numbers
    # in the report still point at the right place.
    # ---------------------------------------------------------------------
    $stripped = [regex]::Replace($text, '/\*.*?\*/', {
            param($m)
            ($m.Value -split "`r?`n" | ForEach-Object { '' }) -join "`n"
        }, 'Singleline')

    $lines = $stripped -split "`r?`n"
    for ($i = 0; $i -lt $lines.Count; $i++) {
        # And single-line comments too.
        $code = $lines[$i] -replace '//.*$', ''
        foreach ($m in [regex]::Matches($code, 'var\(\s*(--[a-zA-Z0-9-]+)\s*(,)?')) {
            $name = $m.Groups[1].Value
            $rel = $f.FullName.Substring($srcDir.Length + 1)
            $where = "$rel`:$($i + 1)"
            if ($defined.ContainsKey($name)) { continue }
            if ($m.Groups[2].Success) {
                # Has a fallback: the declaration still does something.
                $withFallback.Add("$where  $name")
            } else {
                $missing.Add("$where  $name")
            }
        }
    }
}

# --- report ------------------------------------------------------------------
Write-Output ''
Write-Output ("CSS tokens: {0} files, {1} defined" -f $files.Count, $defined.Count)

if ($withFallback.Count -gt 0) {
    # Not a failure -- but printed, because each one is a place where the
    # author knew the token might not exist. Those are worth a look once.
    Write-Output ''
    Write-Output ("NOTE  {0} use(s) rely on a var() fallback (not a failure):" -f $withFallback.Count)
    foreach ($x in ($withFallback | Sort-Object -Unique)) { Write-Output ("      " + $x) }
}

if ($missing.Count -gt 0) {
    Write-Output ''
    Write-Output ("FAIL  {0} use(s) of an UNDEFINED token -- those declarations are silently dropped:" -f $missing.Count)
    foreach ($x in ($missing | Sort-Object -Unique)) { Write-Output ("      " + $x) }
    Write-Output ''
    Write-Output 'Define them in web/src/tokens.css (or fix the name). See the comment'
    Write-Output 'above --font-weight-regular in tokens.css for why this check exists.'
    exit 1
}

Write-Output 'OK: every var(--token) in web/src resolves to a definition.'
exit 0
