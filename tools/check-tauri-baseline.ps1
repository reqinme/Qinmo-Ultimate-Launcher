# =============================================================================
# Tauri security baseline  (plan section 5.8)
# =============================================================================
#
# WHY THIS EXISTS
#
# Section 5.8 says, verbatim:
#
#   > "Front-end zero business logic" is a DESIGN INTENT, not a SECURITY BOUNDARY.
#   > Once the WebView2 page has injected content (a rendered untrusted mod
#   > description, an announcement, Markdown, an external icon), the attack
#   > surface is the ENTIRE Tauri command layer. So there must be enforcement.
#
# And it lists two measures:
#
#   1. CSP not loosened -- production forbids 'unsafe-inline' / 'unsafe-eval';
#      external resources are allow-listed; NO remote scripts are loaded.
#   2. Tauri capabilities least privilege -- per window / per module; NO broad
#      `fs` / `shell` / `http` capability by default; file operations go through
#      CUSTOM COMMANDS, not the generic fs plugin.
#
# A rule that is only written in a doc is not enforced. So it is checked here.
#
# WHAT IT DELIBERATELY DOES NOT DO
#
# It does not require tauri.conf.json to exist yet. Right now `src-tauri/` is
# config-only (the Rust shell needs the Tauri CLI and a large dependency tree,
# and that is a separate decision). A check that hard-failed on the missing
# shell would be red for the wrong reason and would get ignored.
#
# So: when the config IS present, every rule is enforced; when it is absent,
# this says so plainly and exits 0.
#
# -----------------------------------------------------------------------------

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# The repo root is found by walking up for `.git` -- NOT by "up N levels".
# That mistake has been made before: a script moved one directory deeper
# silently started scanning the wrong tree.
$dir = $PSScriptRoot
while ($dir -and -not (Test-Path (Join-Path $dir '.git'))) {
    $dir = Split-Path $dir -Parent
}
if (-not $dir) { throw 'check-tauri-baseline: could not find .git walking up from the script' }

$conf = Join-Path $dir 'src-tauri\tauri.conf.json'
$caps = Join-Path $dir 'src-tauri\capabilities'
$html = Join-Path $dir 'web\index.html'

$problems = New-Object System.Collections.Generic.List[string]
$checked = 0

function Note([string]$msg) { Write-Output ("  " + $msg) }
function Fail([string]$msg) { $problems.Add($msg); Write-Output ("  FAIL  " + $msg) }
function Pass([string]$msg) { Write-Output ("  ok    " + $msg) }

Write-Output '== Tauri security baseline (plan 5.8) =='
Write-Output ''

if (-not (Test-Path $conf)) {
    Write-Output '  src-tauri/tauri.conf.json is not present yet.'
    Write-Output '  The Rust shell is a separate decision (Tauri CLI + a large dependency'
    Write-Output '  tree). The baseline applies the moment the config lands.'
    Write-Output ''
    Write-Output 'All checks passed (nothing to check).'
    exit 0
}

# -----------------------------------------------------------------------------
# 1. CSP must not be loosened.
# -----------------------------------------------------------------------------
$c = Get-Content $conf -Raw -Encoding UTF8 | ConvertFrom-Json
$csp = $null
try { $csp = $c.app.security.csp } catch { $csp = $null }

if ([string]::IsNullOrWhiteSpace($csp)) {
    Fail 'app.security.csp is missing or empty -- a launcher must ship a CSP'
} else {
    $checked++
    if ($csp -match 'unsafe-inline') { Fail "CSP allows 'unsafe-inline': $csp" }
    else { Pass "CSP does not allow 'unsafe-inline'" }

    if ($csp -match 'unsafe-eval') { Fail "CSP allows 'unsafe-eval': $csp" }
    else { Pass "CSP does not allow 'unsafe-eval'" }

    # Remote scripts: any http(s) origin in a -src directive is a remote script
    # (or style/font) load. Section 5.8 forbids remote scripts outright, and
    # this repo has no need for remote anything -- every asset is bundled.
    if ($csp -match '(script-src|style-src|font-src|img-src|connect-src)[^;]*https?://') {
        Fail "CSP allow-lists a remote origin in a -src directive: $csp"
    } else {
        Pass 'CSP allow-lists no remote origin'
    }

    # default-src 'none' is the strictest useful starting point: everything else
    # has to be granted explicitly.
    if ($csp -notmatch "default-src\s+'none'") {
        Note "note: default-src is not 'none' -- not a failure, but review it"
    }
}

# -----------------------------------------------------------------------------
# 2. Capabilities must be least-privilege.
# -----------------------------------------------------------------------------
# These are the plugin prefixes section 5.8 names explicitly.
$banned = @('fs:', 'shell:', 'http:', 'process:', 'dialog:')

if (-not (Test-Path $caps)) {
    Fail 'src-tauri/capabilities/ is missing -- capabilities are where least privilege lives'
} else {
    # Wrapped in @() because StrictMode makes a single FileInfo have no .Count.
    $files = @(Get-ChildItem $caps -Filter '*.json' -File -ErrorAction SilentlyContinue)
    if ($files.Count -eq 0) {
        Fail 'src-tauri/capabilities/ has no .json capability file'
    } else {
        foreach ($f in $files) {
            $checked++
            $raw = Get-Content $f.FullName -Raw -Encoding UTF8
            $json = $raw | ConvertFrom-Json
            $perms = @()
            try { if ($json.permissions) { $perms = @($json.permissions) } } catch { $perms = @() }

            $hit = @()
            foreach ($p in $perms) {
                foreach ($b in $banned) {
                    if ("$p".StartsWith($b)) { $hit += "$p" }
                }
            }
            if ($hit.Count -gt 0) {
                Fail ("$($f.Name) grants broad plugin permission(s): " + ($hit -join ', '))
            } else {
                Pass "$($f.Name) grants no broad fs/shell/http/process/dialog permission"
            }

            # A wildcard scope is the other way this leaks: `"scope": ["**"]`.
            if ($raw -match '"\s*scope\s*"\s*:\s*\[\s*"\*\*"\s*\]') {
                Fail "$($f.Name) has a wildcard scope (\"**\")"
            }
            if ($raw -match '"\s*permissions\s*"\s*:\s*\[\s*"\*"\s*\]') {
                Fail "$($f.Name) grants the wildcard permission `"*`""
            }
        }
    }
}

# -----------------------------------------------------------------------------
# 3. The HTML shell must not load anything remote.
# -----------------------------------------------------------------------------
if (Test-Path $html) {
    $checked++
    $h = Get-Content $html -Raw -Encoding UTF8
    if ($h -match '(src|href)\s*=\s*["'']https?://') {
        Fail 'web/index.html references a remote (src|href) URL'
    } else {
        Pass 'web/index.html references no remote URL'
    }
    if ($h -match '<script(?![^>]*\bsrc=)[^>]*>') {
        # An inline <script> would need 'unsafe-inline' in the CSP, and the CSP
        # above forbids it -- so this pair of checks is what makes "no inline
        # script" actually hold.
        Fail 'web/index.html has an inline <script> (the CSP forbids unsafe-inline, so it could not run)'
    } else {
        Pass 'web/index.html has no inline <script>'
    }
} else {
    Fail 'web/index.html not found'
}

# -----------------------------------------------------------------------------
# 4. The Rust shell must not pull the generic plugins the baseline forbids.
# -----------------------------------------------------------------------------
$cargo = Join-Path $dir 'src-tauri\Cargo.toml'
if (Test-Path $cargo) {
    $checked++
    # Strip comment lines first.
    #
    # A "match the raw file" version of this check is a FALSE POSITIVE waiting to
    # happen -- and it happened: `src-tauri/Cargo.toml` documents, in a comment,
    # that it deliberately does NOT depend on `tauri-plugin-fs`, and the check
    # flagged that comment as a dependency.
    #
    # The rule is about what is DEPENDED ON, so it must look at code, not prose.
    $ct = (Get-Content $cargo -Encoding UTF8 | Where-Object { $_ -notmatch '^\s*#' }) -join "`n"
    $bad_crates = @('tauri-plugin-fs', 'tauri-plugin-shell', 'tauri-plugin-http', 'tauri-plugin-process')
    $found = @()
    foreach ($bc in $bad_crates) { if ($ct -match [regex]::Escape($bc)) { $found += $bc } }
    if ($found.Count -gt 0) {
        Fail ("src-tauri/Cargo.toml depends on the generic plugin(s): " + ($found -join ', ') +
              ' -- section 5.8 says file operations go through CUSTOM commands')
    } else {
        Pass 'src-tauri/Cargo.toml has no generic fs/shell/http/process plugin'
    }
} else {
    Note 'src-tauri/Cargo.toml not present yet (the Rust shell is a later decision)'
}

# -----------------------------------------------------------------------------

Write-Output ''
if ($problems.Count -gt 0) {
    Write-Output ("FAILED: {0} problem(s)" -f $problems.Count)
    foreach ($p in $problems) { Write-Output ("  - " + $p) }
    exit 1
}
Write-Output ("All checks passed ({0} checked)." -f $checked)
exit 0
