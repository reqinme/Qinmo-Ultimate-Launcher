# Negative-control probes for tools/scan-core-vocabulary.ps1
#
# WHY THIS FILE EXISTS:
#
#   A checker that never fires is indistinguishable from a checker that works.
#   This script injects known violations into a real kernel file, asserts the
#   checker catches them, injects known NON-violations, and asserts the checker
#   stays silent -- then restores the file.
#
#   Every one of the three real bugs in the checker was found by a probe, not by
#   reading the code. That is why the probes are kept rather than deleted.
#
# NOTE (project rule): this file must stay ASCII-only. A .ps1 containing
# non-ASCII gets mangled by the console codepage, and the earlier version of
# this very file failed to parse for exactly that reason.
#
# Usage: pwsh -File tools/_probe-scan.ps1
# Exit 0 = all probes behaved as expected.

$ErrorActionPreference = 'Stop'
$root = (Get-Location).Path
if (-not (Test-Path (Join-Path $root '.git'))) { $root = Split-Path $PSScriptRoot -Parent }
$f = Join-Path $root 'crates\qul-core\src\caps.rs'
if (-not (Test-Path $f)) { Write-Output "FAIL: cannot find $f"; exit 2 }
$orig = [System.IO.File]::ReadAllText($f, [Text.Encoding]::UTF8)
$enc = New-Object System.Text.UTF8Encoding($false)
$Q = [char]34

# `require` lists tokens/authorities that MUST appear among the reported hits.
# The first version asserted only the number of violations, and that was too
# weak in one direction and too brittle in the other:
#   - too weak: it passed on any single hit, so it could not tell a real catch
#     from a coincidental one;
#   - too brittle: `piston-meta.mojang.com/mc/x.json` legitimately produces
#     THREE hits (two authorities: three hits: the host in the URL and the same host in the toml string,
#     plus the `piston` identifier token). Pinning the count made the test wrong
#     about correct behaviour.
# So probes now assert "did it catch THIS", not "how many".
$probes = @(
    @{ n = 'A production code names a product (identifier)'; require = @('mojang')
       body = "`n/// x`npub fn mojang_check() -> bool { true }`n" }
    @{ n = 'B production code names a product (string literal)'; require = @('minecraft')
       body = "`n/// x`npub const CORE_DIR: &str = ${Q}minecraft${Q};`n" }
    @{ n = 'C production code hardcodes a host'; require = @('piston-meta.mojang.com')
       body = "`n/// x`npub const META: &str = ${Q}https://piston-meta.mojang.com/mc/x.json${Q};`n" }
    @{ n = 'C2 camel-case product identifier is caught'; require = @('mojang')
       body = "`n/// x`npub struct MojangApproval;`n" }
    @{ n = 'D test code may name products (MUST stay silent)'; require = @()
       body = "`n#[cfg(test)]`nmod probe_tests {`n    #[test]`n    fn t() {`n        let _ = ${Q}https://piston-meta.mojang.com/mc/${Q};`n        let _ = minecraft_thing();`n    }`n    fn minecraft_thing() -> u8 { 1 }`n}`n" }
    @{ n = 'E comments may name products (MUST stay silent)'; require = @()
       body = "`n// Minecraft zip files carry the length in the central directory`n/// see the Mojang manifest`n" }
    @{ n = 'F lookalike words are not false positives (MUST stay silent)'; require = @()
       body = "`n/// x`npub fn forget() {}`npub fn forged() {}`npub fn resorted() {}`n" }
    @{ n = 'F2 uppercase constants ARE caught (underscore branch)'; require = @('mojang')
       body = "`n/// x`npub const MOJANG_DIR: &str = ${Q}x${Q};`n" }
    @{ n = 'F3 known limit: all-caps glued word is a false positive'; require = @('quilt')
       body = "`n/// x`npub const QUILTED: u8 = 1;`n" }
    @{ n = 'G scheme prefix is not an endpoint (MUST stay silent)'; require = @()
       body = "`n/// x`npub fn strip(url: &str) -> &str { url.strip_prefix(${Q}https://${Q}).unwrap_or(url) }`n" }
)

$pass = 0
$fail = 0
foreach ($p in $probes) {
    [System.IO.File]::WriteAllText($f, $orig + $p.body, $enc)
    $out = & (Join-Path $root 'tools\scan-core-vocabulary.ps1') -ShowAll 2>&1
    $code = $LASTEXITCODE
    $got = 0
    $hit = ($out | Select-String -Pattern 'VIOLATIONS: (\d+)')
    if ($hit) { $got = [int]$hit.Matches[0].Groups[1].Value }

    $found = @()
    foreach ($m in [regex]::Matches(($out -join "`n"), '(?m)^\s+token\s+:\s*(.+?)\s*$')) {
        $found += $m.Groups[1].Value
    }
    $missing = @()
    foreach ($r in $p.require) {
        if (-not ($found | Where-Object { $_ -eq $r })) { $missing += $r }
    }
    # A probe with no `require` must produce no hits at all.
    if ($p.require.Count -eq 0) { $ok = ($got -eq 0) } else { $ok = ($missing.Count -eq 0) }

    if ($ok) { $pass++ } else { $fail++ }
    Write-Output ("  {0}  {1}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $p.n)
    Write-Output ("        exit={0} hits={1} found=[{2}]" -f $code, $got, ($found -join ', '))
    if ($missing.Count -gt 0) { Write-Output ("        MISSING: {0}" -f ($missing -join ', ')) }
}
[System.IO.File]::WriteAllText($f, $orig, $enc)

$leftover = (Select-String -Path $f -Pattern 'probe_tests|mojang_check|CORE_DIR|forged|QUILTED' | Measure-Object).Count
Write-Output ''
Write-Output ("  probes: {0} passed / {1} failed" -f $pass, $fail)
Write-Output ("  restore check: leftover probe text = {0}" -f $leftover)
if ($leftover -ne 0) {
    Write-Output '  FAIL: the probe file was not restored -- caps.rs is now dirty'
    exit 2
}
if ($fail -gt 0) { exit 1 }
exit 0
