# =============================================================================
# scan-core-vocabulary.ps1 -- enforce the project's FIRST architecture rule
#
# THE RULE (plan section 4.3):
#
#   The kernel (`qul-core`) must contain ZERO product vocabulary.
#   It does not know what "Minecraft" is; it does not know what a window is.
#
# WHY THIS SCRIPT EXISTS:
#
#   That rule is the load-bearing one for the whole architecture -- it is what
#   makes "adding a product = adding a Provider crate, with zero changes to the
#   kernel and zero changes to the UI" true rather than aspirational.
#
#   It was stated in prose and audited BY EYE. Eye-auditing is how a rule like
#   this dies: it holds for the first few months and then quietly stops holding.
#   This script makes it a fact a command can check.
#
# THE DISTINCTION THAT MATTERS:
#
#   A product name in a COMMENT is GOOD. Our docs must be able to say
#   "Minecraft's zip files all carry the length in the central directory" --
#   that is an explanation, and removing it would make the code worse.
#
#   A product name in PRODUCTION CODE OR A STRING LITERAL is BAD. That is the
#   kernel knowing a product.
#
# -----------------------------------------------------------------------------
# HOW THE TEST-REGION DETECTION WORKS, AND WHY IT IS WRITTEN THIS WAY
#
# Three bugs were found here in sequence, each by a negative-control probe.
# They are recorded because the fixes are what make the current shape correct:
#
#   1. Scanning line by line while carrying an "am I in tests?" flag, and only
#      re-checking that flag on lines containing `{`, meant the flag NEVER
#      reset -- the line that closes `mod tests` contains no `{`. Every line
#      after the test module was therefore silently skipped, and the script
#      reported OK on files it had not really read. This is the worst possible
#      failure for a checker.
#
#   2. A guard added for (1) -- "if `#[cfg(test)]` has no braces, do not stay in
#      test mode" -- then fired on identity.rs:399, where a bare `#[cfg(test)]`
#      is followed by doc comments and THEN the module. Test mode switched off
#      and the script reported 46 violations, every one of them test code.
#
#   3. After replacing the flag with a relative-depth counter, the counter was
#      updated BEFORE the module line was recognised, so the module's own
#      opening brace was missed and the region never closed.
#
# The lesson, which is why the code below looks like this: a stateful
# line-by-line scan is the wrong shape when the property is REGIONAL. So this
# version does it in passes:
#
#   pass 1: for every line, the brace depth AFTER that line, plus its code part
#   pass 2: a line is test code iff it lies strictly inside a
#           `mod <name> { ... }` block whose opening line ends in `{`
#   pass 3: judge only what is left
#
# No ordering dependency, no "did I reset the flag" question.
#
# WHAT IT CANNOT DO:
#
#   It cannot tell whether an IDENTIFIER names a product concept without using
#   the product's word. `ApprovalState` instead of `MojangApproval` is correct
#   and this script cannot know that; that is what review is for. This script
#   is a floor, not a ceiling.
#
# Usage:
#   pwsh -File tools/scan-core-vocabulary.ps1
#   pwsh -File tools/scan-core-vocabulary.ps1 -ShowAll
# Exit code: 0 = clean, 1 = violations, 2 = could not run
# =============================================================================

param(
    [switch]$ShowAll
)

$ErrorActionPreference = 'Stop'

# Repo root: walk UP for .git. Never "up N levels" -- that bug already bit once
# (two levels up = spikes/, which is not the repo root).
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $parent = Split-Path $dir -Parent
    if ($parent -eq $dir) { $dir = $null } else { $dir = $parent }
}
if (-not $dir) {
    Write-Output 'FAIL: could not find the repo root (no .git above tools/)'
    exit 2
}

$coreSrc = Join-Path $dir 'crates/qul-core/src'
if (-not (Test-Path $coreSrc)) {
    Write-Output ("FAIL: cannot find {0}" -f $coreSrc)
    exit 2
}

# -----------------------------------------------------------------------------
# The forbidden list.
#
# Each entry: the token, and WHERE it would legitimately live. That second
# column is what makes a hit actionable rather than just "a word appeared".
#
# The list must stay NARROW. A checker that flags everything gets switched off,
# and then it protects nothing.
# -----------------------------------------------------------------------------
$forbidden = @(
    @{ t = 'minecraft';  belongs = 'the Java/Bedrock Provider crates' }
    @{ t = 'mojang';     belongs = 'the Java Provider crate (or config)' }
    @{ t = 'bedrock';    belongs = 'the Bedrock Provider crate' }
    @{ t = 'forge';      belongs = 'the loader Provider crates' }
    @{ t = 'neoforge';   belongs = 'the loader Provider crates' }
    @{ t = 'fabric';     belongs = 'the loader Provider crates' }
    @{ t = 'quilt';      belongs = 'the loader Provider crates' }
    @{ t = 'curseforge'; belongs = 'the content Provider crates' }
    @{ t = 'modrinth';   belongs = 'the content Provider crates' }
    @{ t = 'bmclapi';    belongs = 'a mirror rule (config data)' }
    @{ t = 'piston';     belongs = 'a mirror rule (config data)' }
    @{ t = 'xbox';       belongs = 'the identity Provider' }
    @{ t = 'microsoft';  belongs = 'the identity Provider' }
)

# Strip a trailing `//` comment, respecting string and char literals.
function Get-CodePart {
    param([string]$line)
    $sb = New-Object System.Text.StringBuilder
    $inStr = $false
    $inChar = $false
    $esc = $false
    for ($i = 0; $i -lt $line.Length; $i++) {
        $c = $line[$i]
        if ($esc) { [void]$sb.Append($c); $esc = $false; continue }
        if ($c -eq '\') { [void]$sb.Append($c); $esc = $true; continue }
        if ($inStr) {
            [void]$sb.Append($c)
            if ($c -eq '"') { $inStr = $false }
            continue
        }
        if ($inChar) {
            [void]$sb.Append($c)
            if ($c -eq "'") { $inChar = $false }
            continue
        }
        if ($c -eq '"') { $inStr = $true; [void]$sb.Append($c); continue }
        if ($c -eq "'") { $inChar = $true; [void]$sb.Append($c); continue }
        if ($c -eq '/' -and $i + 1 -lt $line.Length -and $line[$i + 1] -eq '/') { break }
        [void]$sb.Append($c)
    }
    return $sb.ToString()
}

function Count-Braces {
    param([string]$code)
    $o = ([regex]::Matches($code, '\{')).Count
    $c = ([regex]::Matches($code, '\}')).Count
    return @($o, $c)
}

$violations = @()
$scanned = 0
$productionLines = 0
$testRegions = 0
$files = Get-ChildItem -LiteralPath $coreSrc -Filter '*.rs' -Recurse | Sort-Object FullName

foreach ($f in $files) {
    $rel = $f.FullName.Substring($dir.Length).TrimStart('\')
    $lines = [System.IO.File]::ReadAllLines($f.FullName, [Text.Encoding]::UTF8)
    if ($lines.Count -eq 0) { continue }

    # ---- pass 1: comment flags, code parts, brace depth after each line -----
    $isComment = New-Object bool[] $lines.Count
    $codePart = New-Object string[] $lines.Count
    $depthAfter = New-Object int[] $lines.Count
    $depth = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $t = $lines[$i].TrimStart()
        $isComment[$i] = $t.StartsWith('//')
        $code = ''
        if (-not $isComment[$i]) { $code = Get-CodePart $lines[$i] }
        $codePart[$i] = $code
        $bc = Count-Braces $code
        $depth += ($bc[0] - $bc[1])
        $depthAfter[$i] = $depth
    }

    # ---- pass 2: mark lines inside a `mod <name> {` block -------------------
    # Requiring the exact shape (`mod x {` at end of line) is deliberate: an
    # ambiguous signal is what produced all three bugs documented above.
    $inTestRegion = New-Object bool[] $lines.Count
    $i = 0
    while ($i -lt $lines.Count) {
        $t = $lines[$i].Trim()
        if ($t -match '^mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{\s*$') {
            $testRegions++
            $openDepth = $depthAfter[$i]
            $j = $i + 1
            while ($j -lt $lines.Count -and $depthAfter[$j] -ge $openDepth) { $j++ }
            for ($k = $i + 1; $k -lt $j -and $k -lt $lines.Count; $k++) {
                $inTestRegion[$k] = $true
            }
            $i = $j
        } else {
            $i++
        }
    }

    # ---- pass 3: judge ------------------------------------------------------
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        $no = $i + 1
        $scanned++
        if ($isComment[$i]) { continue }        # comments MAY name products
        if ($inTestRegion[$i]) { continue }     # tests MAY name products
        $productionLines++

        $code = $codePart[$i]

        # Token check.
        #
        # The rule is "the forbidden word appears as a WHOLE IDENTIFIER COMPONENT",
        # not "the identifier equals the word". The first version required exact
        # equality, and the negative-control probe caught it: `mojang_check` was
        # NOT flagged. That is exactly the identifier shape this rule exists to
        # catch (an earlier revision of this repo literally had `MojangApproval`
        # and `serves_bedrock`), so exact matching would have made the checker
        # blind to the most likely violation.
        #
        # at a real component boundary:
        #   eq      : "forge"
        #   prefix  : "mojang_check",  "bedrock_main"
        #   suffix  : "is_minecraft",  "dir_fabric"
        #   camel   : "MojangApproval"   (lowercased -> "mojangapproval")
        # and NOT at a boundary:
        #   "forget"      (forge + t, no separator)   -> not flagged
        #   "forged"      (forge + d)                 -> not flagged
        #   "quilted"     (quilt + ed)                -> not flagged
        #
        # The camel case is why there is a fourth form: after lowercasing we
        # cannot see the case boundary, so "starts with the word AND the next
        # char continues a word" is accepted. That is deliberate -- a product
        # name glued to another word in camel case is still the product name.
        #
        # ---------------------------------------------------------------------
        # STRING LITERALS ARE EXEMPT FROM THE *PARTIAL* MATCH, BUT NOT FROM THE
        # *EXACT* MATCH. Getting this line right took two attempts, and the
        # probe suite caught the first one:
        #
        #   attempt 1: exempt string literals entirely.
        #     -> probe B ("a product name in a string literal") stopped firing.
        #        That probe exists because a hardcoded product STRING is a real
        #        violation (`launcher_brand = "minecraft"`), so exempting all
        #        strings removed a check that was doing work.
        #
        #   attempt 2 (this one): exempt strings only from the PARTIAL match.
        #     -> `"minecraft"` alone is still flagged (it is the product name),
        #        while `rename = "minecraftArguments"` is not (it is a protocol
        #        key we are required to spell verbatim, and serde's rename
        #        attribute cannot take a `const`).
        #
        # The distinction is "did we NAME something, or did we SPELL a key?".
        # An exact-match string is a name we chose. A partial-match string is
        # spelling inside a longer word that is not ours.
        foreach ($m in [regex]::Matches($code, '[A-Za-z_][A-Za-z0-9_]*')) {
            $inString = ($m.Index -gt 0 -and ($code[$m.Index - 1] -eq '"' -or $code[$m.Index - 1] -eq "'"))
            $orig = $m.Value
            $low = $orig.ToLowerInvariant()
            foreach ($fb in $forbidden) {
                $w = $fb.t
                $hit = $false
                if ($low -eq $w) {
                    $hit = $true
                } elseif ($inString) {
                    # Partial match inside a string literal: that is spelling a
                    # protocol key, not naming a product. See the long note above.
                    $hit = $false
                } elseif ($low.StartsWith($w + '_') -or $low.EndsWith('_' + $w)) {
                    $hit = $true
                } elseif ($low.StartsWith($w) -and $low.Length -gt $w.Length) {
                    # camelCase / PascalCase boundary, e.g. MojangApproval.
                    #
                    # This branch requires a CAPITAL in the ORIGINAL token right
                    # after the word -- i.e. a real camelCase boundary.
                    #
                    # Two earlier versions of this branch were wrong, and both
                    # were caught by the probe suite rather than by reading:
                    #
                    #   v1 accepted ANY continuation -> `forget` and `forged`
                    #      were reported as naming `forge`.
                    #   v2 accepted a capital OR a digit -> `QUILTED` (all caps)
                    #      was still reported as naming `quilt`.
                    #
                    # Capital-only is the version that satisfies both directions:
                    # it catches `MojangApproval` (the shape this rule exists
                    # for) and clears `forget` and `forged`.
                    #
                    # KNOWN LIMITATION, not a bug to be "fixed" later by
                    # loosening the rule: `QUILTED` is still reported, because
                    # `QUILTED[5]` is `E` -- QUILT + ED -- and we cannot tell an
                    # all-caps glued word from an all-caps camel boundary.
                    #
                    # The trade is deliberate and was measured: the common real
                    # shapes are underscore-separated (`MOJANG_DIR`,
                    # `MINECRAFT_HOME`, `CURSEFORGE_API`, `PISTON_META`), and ALL
                    # of those are caught by the underscore branch above. So the
                    # cost of this limitation is a rare false positive on an
                    # all-caps text word, while the benefit is catching the
                    # camel-case identifier shape that this repo actually had
                    # (`MojangApproval`, `serves_bedrock`).
                    #
                    # This is not cosmetic. "forge" and "quilt" are ordinary
                    # English fragments, and a checker that flags `forget`
                    # teaches the team to ignore its output -- at which point it
                    # protects nothing at all.
                    $nxt = $orig[$w.Length]
                    $hit = [char]::IsUpper($nxt)
                }
                if ($hit) {
                    $violations += [pscustomobject]@{
                        file = $rel; line = $no; token = $fb.t
                        belongs = $fb.belongs
                        text = $line.Trim()
                        kind = 'production code names a product'
                    }
                }
            }
        }

        # A HOST LITERAL in a PRODUCTION string is a hardcoded endpoint. Per the
        # config spec, no product endpoint may be a compile-time constant in the
        # kernel.
        #
        # The first version matched any `"..://"` and therefore flagged
        # `strip_prefix("https://")` in timeout.rs twice. That is a SCHEME
        # PREFIX -- generic parsing logic, not an endpoint. So the test is now:
        # is there a host-looking authority after `://`? A host contains a dot
        # with a letter after it, so `"https://"`, `"http://"` and `"file://"`
        # are all clean.
        foreach ($m in [regex]::Matches($code, '"[^"]*://([^"/]+)')) {
            $authority = $m.Groups[1].Value
            if ($authority -match '[A-Za-z0-9-]\.[A-Za-z]') {
                $violations += [pscustomobject]@{
                    file = $rel; line = $no; token = $authority
                    belongs = 'config data (see docs/config spec: no product endpoint is a compile-time constant)'
                    text = $line.Trim()
                    kind = 'hardcoded host in production code'
                }
            }
        }
    }
}

Write-Output '== scan-core-vocabulary: does the kernel know any product? =='
Write-Output ''
Write-Output '  rule      : crates/qul-core must contain ZERO product vocabulary in production code'
Write-Output ("  files     : {0}" -f $files.Count)
Write-Output ("  lines     : {0} scanned, {1} judged (the rest are comments or test code)" -f $scanned, $productionLines)
Write-Output ("  test mods : {0} regions treated as test code" -f $testRegions)
Write-Output ("  forbidden : {0} tokens + hardcoded host literals" -f $forbidden.Count)
Write-Output ''

if ($violations.Count -eq 0) {
    Write-Output 'OK: the kernel names no product.'
    Write-Output ''
    Write-Output '  What this does NOT prove: it cannot see a product concept that is'
    Write-Output '  named without the product word -- e.g. an identifier spelled'
    Write-Output '  generically while meaning one specific product. That is what review'
    Write-Output '  is for. This script is a floor, not a ceiling.'
    exit 0
}

Write-Output ("VIOLATIONS: {0}" -f $violations.Count)
Write-Output ''
$shown = 0
foreach ($v in $violations) {
    if (-not $ShowAll -and $shown -ge 40) {
        Write-Output ("  ... and {0} more (use -ShowAll)" -f ($violations.Count - $shown))
        break
    }
    Write-Output ("  {0}:{1}  [{2}]" -f $v.file, $v.line, $v.kind)
    Write-Output ("      token   : {0}" -f $v.token)
    Write-Output ("      belongs : {0}" -f $v.belongs)
    Write-Output ("      line    : {0}" -f $v.text)
    $shown++
}
Write-Output ''
Write-Output 'How to fix, by case:'
Write-Output '  1. The value is product-specific -> make it a caller-supplied parameter'
Write-Output '     or config data. (Precedent: identity.rs takes `what: &str`;'
Write-Output '     http.rs takes `header_name: &str` instead of a mirror header name.)'
Write-Output '  2. It is genuinely generic -> you have found a false positive. Fix the'
Write-Output '     CHECK, not the exemption list. Widening the exemption list weakens'
Write-Output '     the check for everyone; fixing the check keeps it sharp.'
Write-Output '  3. It is a test fixture -> move the line into `mod tests`, where naming'
Write-Output '     products is correct.'
exit 1
