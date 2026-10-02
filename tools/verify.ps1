# =============================================================================
# verify.ps1 -- ONE command that runs EVERY check the project relies on.
#
# WHY THIS FILE EXISTS:
#
#   Before this existed, "is the project green?" meant remembering and typing
#   five commands in the right order:
#
#       cargo fmt --all --check
#       cargo clippy --workspace --all-targets -- -D warnings
#       cargo test --workspace
#       pwsh tools/check-docs.ps1 -Strict
#       pwsh tools/audit-milestone.ps1
#
#   ...plus whatever checks had been added recently. That is a real problem, not
#   a convenience issue: a check you have to REMEMBER is a check that will be
#   skipped, and a skipped check is indistinguishable from a passing one.
#
#   It is also how the previous round went wrong: a whole class of checking was
#   assumed absent because nobody had a single place to look.
#
# WHAT IT DOES NOT DO:
#
#   It does not judge. It runs each check, prints its own output, and reports a
#   summary. Everything it runs is deterministic and local -- no network, no
#   build artifacts beyond what cargo already makes.
#
# NOTE (project rule): this file must stay ASCII-only. A .ps1 containing
# non-ASCII gets mangled by the console codepage, and an earlier probe script
# failed to parse for exactly that reason.
#
# Usage:
#   pwsh -File tools/verify.ps1              # everything
#   pwsh -File tools/verify.ps1 -Fast        # skip cargo test (the slow one)
#   pwsh -File tools/verify.ps1 -List        # show the checks, run none
# Exit code: 0 = all green, 1 = at least one failed, 2 = could not run
# =============================================================================

param(
    [switch]$Fast,
    [switch]$List,
    # Skip the worktree-clean check. This is a REAL use case, not a convenience:
    # "run everything before I commit" is when verification is most valuable,
    # and at that moment the worktree is dirty BY DEFINITION. Without this flag
    # the natural workflow is to commit first and verify afterwards, which is
    # the wrong order.
    #
    # It does not weaken anything: CI always runs with the worktree clean,
    # because CI checks out a commit.
    [switch]$AllowDirty
)

$ErrorActionPreference = 'Continue'

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

# Make cargo available the same way a fresh shell would find it.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

# Each check: name, what it protects against, and how to run it.
#
# The `protects` field is not decoration. A check whose purpose is not written
# down becomes a check nobody dares delete -- and then it stays forever even
# after the thing it guarded is gone.
$checks = @(
    @{ key = 'fmt'
       name = 'Rust formatting'
       protects = 'style diffs that hide real changes'
       kind = 'cmd'; run = { cargo fmt --all --check } }
    @{ key = 'clippy'
       name = 'Clippy (warnings are errors)'
       protects = 'the MSRV trap (File::lock needs 1.89) and silent lints'
       kind = 'cmd'; run = { cargo clippy --workspace --all-targets -- -D warnings } }
    @{ key = 'test'
       name = 'All tests (unit + integration + architecture guards)'
       protects = 'everything the test files pin'
       kind = 'cmd'; skipWhenFast = $true; run = { cargo test --workspace } }
    @{ key = 'vocab'
       name = 'Kernel vocabulary scanner'
       protects = 'the kernel learning a product (plan section 4.3)'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\scan-core-vocabulary.ps1') } }
    @{ key = 'vocab-probe'
       name = 'Vocabulary scanner negative controls'
       protects = 'the checker silently reporting OK'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\_probe-scan.ps1') } }
    @{ key = 'docs'
       name = 'Doc structure (heading conservation / fences / duplicates)'
       protects = 'an edit eating a heading (happened 4 times)'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-docs.ps1') -Strict } }
    @{ key = 'audit'
       name = 'Milestone audit (refs / promises / licences / counts)'
       protects = 'dangling cross-references and licence drift'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\audit-milestone.ps1') } }
    @{ key = 'web'
       name = 'Frontend (typecheck / lint / tests / build)'
       protects = 'TypeScript strict mode, the three design lint rules, and the component contract tests'
       kind = 'cmd'; run = { pnpm run --silent verify:web } }
    @{ key = 'impeccable'
       name = 'UI anti-pattern detector (repos/impeccable, 61 deterministic rules)'
       protects = 'the M4 acceptance rule (plan section 8): the five categories must score ZERO hits -- generic AI colouring, purple gradients, glow particles, glassmorphism pile-up, SaaS landing-page cliches'
       kind = 'cmd'; run = { node repos/impeccable/cli/bin/cli.js detect web/dist } }
    @{ key = 'tauri'
       name = 'Tauri security baseline (CSP / capabilities least privilege)'
       protects = 'plan section 5.8: the WebView is not a security boundary, so "front end has zero business logic" must be ENFORCED -- no unsafe-inline/unsafe-eval, no remote origin, no broad fs/shell/http capability'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-tauri-baseline.ps1') } }
    @{ key = 'tokens'
       name = 'CSS tokens (every var(--token) resolves to a definition)'
       protects = 'a REAL defect found while writing TitleBar.css: --font-weight-medium and --font-weight-semibold were used in 13 places and defined in 0. The symptom is SILENT -- an undefined custom property makes the whole declaration invalid, so the weight falls back to normal, and on 13px Chinese text nobody notices. --line-height-tight was the same defect.'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-css-tokens.ps1') } }
    @{ key = 'ladder'
       name = 'Material ladder (opacity steps / 1px stroke / 4px whitespace rhythm)'
       protects = 'the M4 acceptance criterion in plan section 8, which says the three-layer material opacity ladder, the 1px stroke, and the whitespace rhythm MUST BE POINTABLE -- not "it looks about right". This prints the actual numbers and checks them against UI spec section 3.1: container panel 8-12%, content card higher than the panel, the overlay opaque, --hairline exactly 1px (and 0.5px at 2x DPI so it stays 1 PHYSICAL pixel), every --space-N a multiple of 4px.'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-material-ladder.ps1') } }
    @{ key = 'css-classes'
       name = 'CSS class ownership (one owner per class; every className resolves)'
       protects = 'two REAL defects found on 2026-10-02, the first time the desktop window was opened. (1) .shell was defined by BOTH web/src/styles.css and web/src/routes/Shell.css, so the whole application shell was laid out by the U0 self-check page: the winner is decided by the import order in main.tsx, the loser does not warn, and the titlebar ended up in a 720px centred column. (2) .panel__title had two owners with DIFFERENT rules, so the self-check page was restyling the component library panel title. It also catches a className that no stylesheet defines -- an unknown class is simply an unstyled element, which is silent in the same way an undefined --token is.'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-css-classes.ps1') } }
    @{ key = 'css-grid'
       name = 'CSS grid (a zero-width track needs explicit placement)'
       protects = 'the second REAL layout defect of 2026-10-02: Shell.css collapses its middle column (grid-template-columns: var(--shell-rail-w) 0 1fr) while the children carried no grid-column, so the grid auto-placed them in DOM order and the main region landed in the 0-width column -- the home page rendered as one character per line and the right half of the window was empty. jsdom has no layout engine, so no vitest case can see it, and tsc/eslint/token checkers are all blind to it. Only a human looking at the real window caught it.'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-css-grid.ps1') } }
    @{ key = 'titlebar'
       name = 'Titlebar contract (event table + glyph names: spec == code)'
       protects = 'the failure mode behind THREE REAL defects: with decorations:false every titlebar behaviour is re-implemented by hand, and what gets forgotten is always ONE ELEMENT x ONE EVENT FAMILY -- or one picture. faa7fa1 excluded pointerdown but not dblclick on the three window controls, so double-clicking CLOSE maximized the window and then closed it. The same shape was found again by contract.test.tsx: a disabled search box never receives React synthetic onDoubleClick, so swallowing it on that element silently did nothing. Then the maximize button drew the WRONG PICTURE for weeks: it rendered the morphicons spike (menu -> close) in that slot, so the spec 4.5.2 square was never drawn at all, and no test noticed because the glyph test asserted the spike paths while TitleBar.test never touched an svg -- the user eye found it. The contract now exists twice (spec 4.5.1 markdown table <-> TITLEBAR_ITEMS in web/src/titlebar/contract.ts) and so do the four glyph names (spec 4.5.2 <-> WINDOW_GLYPH_NAMES in web/src/titlebar/glyphs.tsx); this check fails if the copies disagree in any cell or name, if either side lists an element/name the other does not, if a declared glyph has no shape (it would render an empty svg), or if a row stops parsing (a silent skip would make it check less and less while still printing OK).'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-titlebar-contract.ps1') } }
    @{ key = 'shell'
       name = 'Shell contract (status cells, rail sizes, nav tree, version: spec == code)'
       protects = 'the failure mode of THIS round: the shell is the one screen nothing else can compare against, so the spec and the code drift apart in ways every other check calls green. The three pages whose sub-navigation the spec still called "not designed" (accounts / toolbox / about) had already been built and were known only to the code; the status bar said "Java version" in the spec while the code had language-neutral "runtime"; the seven primary routes had no spec-side statement of their paths at all. Two more shapes this pins down: (a) hrefOf() in Shell.tsx -- a PRIMARY key with no case, or a path that disagrees with the spec, means a rail entry that goes nowhere or two names for one route (the router factory took `path: string` for months, so a sub-page path was never registered in the `to=` union: it compiled and 404-ed at runtime); (b) the rail width -- "56px narrow / 200px wide" is pinned to WHERE the declaration is written (inside a [data-rail="narrow"] block vs outside it), not merely to the file containing those two numbers, so a second override block fails; and because the CSS reader refuses to skip a line that declares a custom property in a shape it cannot read, a new variable cannot be added in a form the size check never sees. It also fails if the version shown on screen (web/src/app/version.ts APP_VERSION) stops matching package.json or src-tauri/tauri.conf.json.'
       kind = 'ps1'; run = { & (Join-Path $dir 'tools\check-shell-contract.ps1') } }
    @{ key = 'clean'
       name = 'Worktree is clean'
       protects = 'generated files being committed by accident'
       kind = 'git'; run = { git -C $dir status --porcelain } }
)

if ($List) {
    Write-Output '== verify.ps1: what it checks =='
    Write-Output ''
    foreach ($c in $checks) {
        $skip = if ($c.skipWhenFast) { '  [skipped with -Fast]' } else { '' }
        Write-Output ("  {0,-11} {1}{2}" -f $c.key, $c.name, $skip)
        Write-Output ("              protects: {0}" -f $c.protects)
    }
    Write-Output ''
    exit 0
}

Write-Output '== verify.ps1: every check the project relies on =='
Write-Output ''
Write-Output ("  repo : {0}" -f $dir)
Write-Output ("  mode : {0}" -f $(if ($Fast) { '-Fast (cargo test skipped)' } else { 'full' }))
Write-Output ''

$results = @()
$failed = 0

foreach ($c in $checks) {
    if ($AllowDirty -and $c.key -eq 'clean') {
        Write-Output ("--- {0}  [{1}]  SKIPPED (-AllowDirty)" -f $c.name, $c.key)
        Write-Output '    (run without -AllowDirty before pushing: CI will check it)'
        Write-Output ''
        $results += [pscustomobject]@{ key = $c.key; status = 'skipped' }
        continue
    }

    if ($Fast -and $c.skipWhenFast) {
        Write-Output ("--- {0}  [{1}]  SKIPPED (-Fast)" -f $c.name, $c.key)
        $results += [pscustomobject]@{ key = $c.key; status = 'skipped' }
        continue
    }

    Write-Output ("--- {0}  [{1}]" -f $c.name, $c.key)
    Write-Output ("    protects: {0}" -f $c.protects)

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $out = & $c.run 2>&1
    $code = $LASTEXITCODE
    $sw.Stop()

    # ---------------------------------------------------------------------
    # ONE retry, for the `test` step only, and it is reported HONESTLY.
    #
    # WHY THIS EXISTS -- a real, observed, non-reproducible failure:
    #
    #   During the round that added the WinHTTP transport, this step failed
    #   once with exit 101, and then passed on the next run. I ran
    #   `cargo test --workspace` five times in a row and
    #   `cargo clippy --all-targets` three times: all green.
    #
    #   The only mechanism on this platform that produces a non-zero exit
    #   without a real test failure is a FILE LOCK: `cargo` relinks the test
    #   executables and `.pdb` files, and on Windows those can be transiently
    #   held by the OS (Defender, the indexer, a just-exited process).
    #   `cargo test --workspace` links many binaries, so it is the step most
    #   exposed to that.
    #
    # WHAT THIS DELIBERATELY DOES NOT DO:
    #
    #   It does not swallow the failure. A retried-and-passed step prints BOTH
    #   attempts, and is counted as `ok-retried` -- a distinct status, so a
    #   persistent problem cannot hide behind one retry. And the retry is not
    #   offered to any other step: `docs` / `audit` / `vocab` failing is a real
    #   finding, not a lock race.
    # ---------------------------------------------------------------------
    $retried = $false
    if ($code -ne 0 -and $c.key -eq 'test') {
        Write-Output ("    first attempt FAILED (exit={0}) -- retrying ONCE (see the note in this script)" -f $code)
        # Print the first attempt's tail even though we retry: if the retry
        # passes, this is the only record of what the flake looked like.
        $t0 = @($out)
        if ($t0.Count -gt 12) { $t0 = $t0[($t0.Count - 12)..($t0.Count - 1)] }
        $t0 | ForEach-Object { Write-Output ("      [attempt 1] {0}" -f $_) }
        $out = & $c.run 2>&1
        $code = $LASTEXITCODE
        $retried = $true
        if ($code -eq 0) {
            Write-Output '    second attempt PASSED -- the first was a transient failure, not a test result'
        }
    }

    # A check that cannot report a status is a broken check, not a passing one.
    # This matters: `check-copy-paste.ps1` deliberately exits non-zero when it
    # cannot run, and the CI workflow has an explicit comment about that. The
    # same discipline applies here.
    if ($c.kind -eq 'git') {
        if ($out) {
            Write-Output '    worktree is NOT clean:'
            $out | ForEach-Object { Write-Output ("      {0}" -f $_) }
            $code = 1
        } else {
            $code = 0
        }
    }

    if ($code -eq 0) {
        if ($retried) {
            Write-Output ("    OK AFTER RETRY  ({0:N1}s)" -f $sw.Elapsed.TotalSeconds)
            $results += [pscustomobject]@{ key = $c.key; status = 'ok-retried' }
        } else {
            Write-Output ("    OK  ({0:N1}s)" -f $sw.Elapsed.TotalSeconds)
            $results += [pscustomobject]@{ key = $c.key; status = 'ok' }
        }
    } else {
        $failed++
        Write-Output ("    FAILED  exit={0}  ({1:N1}s)" -f $code, $sw.Elapsed.TotalSeconds)
        # Print the tail: enough to act on, not so much that the summary scrolls away.
        $tail = @($out)
        if ($tail.Count -gt 30) { $tail = $tail[($tail.Count - 30)..($tail.Count - 1)] }
        $tail | ForEach-Object { Write-Output ("      {0}" -f $_) }
        $results += [pscustomobject]@{ key = $c.key; status = 'failed' }
    }
    Write-Output ''
}

# -----------------------------------------------------------------------------
# Summary.
#
# The point of this block is that a human can read the LAST FEW LINES and know
# whether to look further. A verify script whose verdict requires scrolling is
# a verify script people stop reading.
# -----------------------------------------------------------------------------
$ok = @($results | Where-Object { $_.status -eq 'ok' -or $_.status -eq 'ok-retried' }).Count
$skipped = @($results | Where-Object { $_.status -eq 'skipped' }).Count

Write-Output '== summary =='
foreach ($r in $results) {
    $mark = switch ($r.status) { 'ok' { 'OK  ' } 'ok-retried' { 'OK* ' } 'failed' { 'FAIL' } default { 'skip' } }
    Write-Output ("  {0}  {1}" -f $mark, $r.key)
}
Write-Output ''
Write-Output ("  {0} ok, {1} failed, {2} skipped" -f $ok, $failed, $skipped)
$retriedOnes = @($results | Where-Object { $_.status -eq 'ok-retried' })
if ($retriedOnes.Count -gt 0) {
    Write-Output ''
    Write-Output ("  NOTE: {0} step(s) passed only on the SECOND attempt (marked OK*):" -f $retriedOnes.Count)
    foreach ($r in $retriedOnes) { Write-Output ("        {0}" -f $r.key) }
    Write-Output '        A retry that succeeds is a transient failure, not a passing test.'
    Write-Output '        If this shows up often, the flake needs to be found -- not retried away.'
}

if ($failed -gt 0) {
    Write-Output ''
    Write-Output '  A failing check is not necessarily a broken project. Read the output'
    Write-Output '  above: some checks fail only because something they need (reference'
    Write-Output '  clones, a clean worktree) is absent. What must never happen is a'
    Write-Output '  check being treated as passing when it did not run.'
    exit 1
}

if ($skipped -gt 0) {
    Write-Output ''
    # Name the flag(s) that actually caused the skips. The first version always
    # said "-Fast", which was wrong as soon as -AllowDirty existed -- and a
    # summary that misattributes why something did not run is the same class of
    # problem as a check that silently does not run.
    $why = @()
    if ($Fast) { $why += '-Fast' }
    if ($AllowDirty) { $why += '-AllowDirty' }
    Write-Output ("  NOTE: {0} check(s) were skipped by {1}. Run without it before" -f $skipped, ($why -join ' / '))
    Write-Output '        claiming the project is fully verified (CI runs everything).'
}
Write-Output ''
Write-Output 'All checks passed.'
exit 0
