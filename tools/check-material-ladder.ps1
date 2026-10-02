# =============================================================================
# check-material-ladder.ps1 -- the three-layer material ladder must be provable
# =============================================================================
#
# WHY THIS EXISTS
#
# The M4 acceptance criterion (plan section 8) ends with:
#
#     "the opacity ladder of the three material layers, the 1px stroke, and
#      the whitespace rhythm MUST BE POINTABLE -- not 'it looks about right'."
#
# "Pointable" means a command prints the numbers and something checks them.
# Without this, the ladder is three magic percentages in a 700-line token file,
# and changing 12% to 13% would silently leave the spec range with nothing
# going red.
#
# WHAT THE SPEC SAYS (docs/UI设计规格.md section 3.1, lines 69-74)
#
#     layer            fill                              stroke
#     ---------------- --------------------------------- -----------------
#     window backdrop  DWM material (we do not draw)     system
#     container panel  surface.panel, opacity 8-12%      stroke.subtle, 1px
#     content card     surface.card, opaque or high      stroke.subtle
#     overlay (island) surface.raised, highest contrast  stroke.strong
#
# And the disciplines at lines 76-79:
#
#     - "the opacity ladder is FIXED STEPS, not hand-tuned values"
#     - "the stroke is 1px, not 'about 1px'"
#     - "cards carry NO shadow; shadows are for overlays only"
#
# WHAT IT CHECKS
#
#   1. container panel opacity is inside 8-12%
#   2. content card opacity is HIGHER than the panel (a ladder, not two values)
#   3. the overlay layer is opaque (so it does not depend on what is behind it)
#   4. --hairline is 1px in the default theme, and only 0.5px under the
#      high-DPI media query (spec section 3.3: it must stay 1px PHYSICAL)
#   5. every --space-N is a multiple of 4px
#   6. the spacing scale is monotonically increasing
#
# WHAT IT DOES NOT CHECK (honest limits)
#
#   - It does NOT check that components actually USE the ladder. That is what
#     the component tests and the impeccable detector are for.
#   - It does NOT check contrast. That is a separate concern with its own
#     spec note (see the open question about the 2.85:1 vs 6.10:1 formula).
#   - It reads the DARK theme (:root). The light theme is checked for the
#     LADDER (relative order) but not for the 8-12% range, because the spec
#     only gives that range for the translucent-over-material case.
#
# ASCII-ONLY: this file must stay ASCII.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Walk up for .git rather than counting directory levels -- that mistake has
# been made in this repo.
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $dir = Split-Path $dir -Parent
}
if (-not $dir) { throw 'check-material-ladder: could not find .git walking up from the script' }

$tokensPath = Join-Path $dir 'web\src\tokens.css'
if (-not (Test-Path $tokensPath)) { throw "check-material-ladder: no tokens.css at $tokensPath" }

$text = Get-Content $tokensPath -Raw -Encoding UTF8

# --- helpers -----------------------------------------------------------------

# Returns the alpha of a `rgb(R G B / P%)` value, or $null when the value is
# not translucent (a hex colour, `none`, a `var(...)`, ...).
function Get-AlphaPercent([string]$value) {
    $m = [regex]::Match($value, 'rgb\([^)]*/\s*([0-9.]+)%\s*\)')
    if ($m.Success) { return [double]$m.Groups[1].Value }
    return $null
}

# Every `--name: value;` inside the FIRST `:root { ... }` block, which is the
# dark theme. Later blocks (:root[data-theme=...], prefers-color-scheme, the
# high-DPI media query) are deliberately not merged -- see the header.
function Get-Block([string]$src, [string]$selector) {
    $i = $src.IndexOf($selector)
    if ($i -lt 0) { return $null }
    $open = $src.IndexOf('{', $i)
    if ($open -lt 0) { return $null }
    $depth = 0
    for ($j = $open; $j -lt $src.Length; $j++) {
        $c = $src[$j]
        if ($c -eq '{') { $depth++ }
        elseif ($c -eq '}') {
            $depth--
            if ($depth -eq 0) { return $src.Substring($open + 1, $j - $open - 1) }
        }
    }
    return $null
}

$root = Get-Block $text ':root'
if (-not $root) { throw 'check-material-ladder: could not find the first :root block' }

$vars = @{}
foreach ($m in [regex]::Matches($root, '(?m)^\s*(--[a-zA-Z0-9-]+)\s*:\s*([^;]+);')) {
    $vars[$m.Groups[1].Value] = $m.Groups[2].Value.Trim()
}

$fail = New-Object System.Collections.Generic.List[string]
$notes = New-Object System.Collections.Generic.List[string]

# --- 1 / 2 / 3: the ladder ---------------------------------------------------

$panelRaw = $vars['--surface-panel']
$cardRaw = $vars['--surface-card']
$raisedRaw = $vars['--surface-raised']

$panel = Get-AlphaPercent $panelRaw
$card = Get-AlphaPercent $cardRaw

if ($null -eq $panel) {
    $fail.Add("--surface-panel is not a translucent rgb(.../N%) value: '$panelRaw'")
} elseif ($panel -lt 8 -or $panel -gt 12) {
    $fail.Add("--surface-panel is $panel% but the spec range is 8-12% (UI spec section 3.1)")
}

if ($null -eq $card) {
    $fail.Add("--surface-card is not a translucent rgb(.../N%) value: '$cardRaw'")
} elseif ($null -ne $panel -and $card -le $panel) {
    $fail.Add("--surface-card ($card%) is NOT higher than --surface-panel ($panel%) -- that is not a ladder")
}

# The overlay must be opaque: the island floats over anything, so it must not
# depend on what happens to be behind it.
if ($null -ne (Get-AlphaPercent $raisedRaw)) {
    $fail.Add("--surface-raised is translucent ('$raisedRaw') but the overlay layer must be OPAQUE (spec section 3.1)")
}

# --- 4: the 1px stroke -------------------------------------------------------

$hairline = $vars['--hairline']
if ($hairline -ne '1px') {
    $fail.Add("--hairline is '$hairline' but the spec says 1px, not 'about 1px' (UI spec section 3.1)")
}

# And under high DPI it must become 0.5px so that it stays 1 PHYSICAL pixel.
$dpiBlock = $null
foreach ($m in [regex]::Matches($text, '(?s)@media[^{]*\{')) {
    $seg = $text.Substring($m.Index, [Math]::Min(4000, $text.Length - $m.Index))
    if ($seg -match '--hairline\s*:\s*([0-9.]+px)') {
        $dpiBlock = $Matches[1]
        break
    }
}
if ($null -eq $dpiBlock) {
    $fail.Add('no high-DPI override of --hairline found -- spec section 3.3 requires it to stay 1 PHYSICAL pixel')
} elseif ($dpiBlock -ne '0.5px') {
    $fail.Add("the high-DPI --hairline is '$dpiBlock' but 1 physical px at 2x DPI is 0.5px")
}

# --- 5 / 6: the whitespace rhythm -------------------------------------------

$steps = New-Object System.Collections.Generic.List[object]
foreach ($k in $vars.Keys) {
    if ($k -match '^--space-([0-9]+)$') {
        $raw = $vars[$k]
        if ($raw -match '^([0-9.]+)px$') {
            $steps.Add([pscustomobject]@{ Name = $k; N = [int]$Matches[1]; Px = [double]$Matches[1] * 0 + [double]$Matches[0].Replace('px', '') })
        } elseif ($raw -eq '0') {
            $steps.Add([pscustomobject]@{ Name = $k; N = [int]$Matches[1]; Px = 0.0 })
        } else {
            $fail.Add("$k is '$raw' -- the spacing scale must be plain px values")
        }
    }
}
$sorted = $steps | Sort-Object N

if ($sorted.Count -lt 6) {
    $fail.Add("the spacing scale has only $($sorted.Count) steps -- spec section 3.2 wants a usable ladder")
}

foreach ($s in $sorted) {
    if (($s.Px % 4) -ne 0) {
        $fail.Add("$($s.Name) is $($s.Px)px -- not a multiple of 4px")
    }
}
for ($i = 1; $i -lt $sorted.Count; $i++) {
    if ($sorted[$i].Px -le $sorted[$i - 1].Px) {
        $fail.Add("the spacing scale is not increasing: $($sorted[$i - 1].Name)=$($sorted[$i - 1].Px)px then $($sorted[$i].Name)=$($sorted[$i].Px)px")
    }
}

# --- report ------------------------------------------------------------------

Write-Output ''
Write-Output 'Material ladder (dark theme, the first :root block):'
Write-Output ("  container panel   surface.panel    {0,-22} {1}%" -f $panelRaw, $panel)
Write-Output ("  content card      surface.card     {0,-22} {1}%" -f $cardRaw, $card)
Write-Output ("  overlay           surface.raised   {0,-22} {1}" -f $raisedRaw, $(if ($null -eq (Get-AlphaPercent $raisedRaw)) { 'opaque (correct)' } else { 'TRANSLUCENT' }))
Write-Output ("  stroke            --hairline       {0}" -f $hairline)
if ($null -ne $dpiBlock) {
    Write-Output ("  stroke (2x DPI)   --hairline       {0}" -f $dpiBlock)
}
$listed = ($sorted | ForEach-Object { "$($_.Px)" }) -join ' / '
Write-Output ("  whitespace        --space-0..N     {0} px  (all multiples of 4)" -f $listed)

foreach ($n in $notes) { Write-Output ("NOTE  " + $n) }

if ($fail.Count -gt 0) {
    Write-Output ''
    Write-Output ("FAIL  {0} problem(s) -- the ladder is not pointable:" -f $fail.Count)
    foreach ($x in $fail) { Write-Output ("      " + $x) }
    Write-Output ''
    Write-Output 'The M4 acceptance criterion says these must be POINTABLE, not'
    Write-Output '"it looks about right". Fix the tokens (web/src/tokens.css).'
    exit 1
}

Write-Output ''
Write-Output 'OK: the material ladder, the 1px stroke, and the 4px spacing rhythm all hold.'
exit 0
