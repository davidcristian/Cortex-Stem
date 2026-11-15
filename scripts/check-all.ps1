#!/usr/bin/env pwsh
<#
.SYNOPSIS
    The verification ladder (AGENTS.md section 5) on a Windows host, plus a
    debug Tauri build.

.DESCRIPTION
    Runs the same rungs as scripts/check-all.sh (1, 2, 3, 3b, 3c, 4, 5, 6, 7) and
    then `pnpm tauri build --debug`, which links and bundles the real app: the
    local equivalent of the CI `windows` job (rung 8). Keep the commands in
    sync with scripts/check-all.sh and .github/workflows/ci.yml.
    Works in Windows PowerShell 5.1 and PowerShell 7.

.PARAMETER NoFrontend
    Skip rungs 5-7 and the Tauri build (which needs the frontend bundle).

.PARAMETER NoE2E
    Skip rung 7 (Playwright screenshots).

.PARAMETER NoInstall
    Skip `pnpm install` (when it has already been run).

.PARAMETER NoTauriBuild
    Skip the final `pnpm tauri build --debug`.

.PARAMETER CI
    CI mode: implies -NoInstall when node_modules\ exists, and a missing
    package.json is an error instead of a skip.

.EXAMPLE
    pwsh scripts/check-all.ps1

.EXAMPLE
    pwsh scripts/check-all.ps1 -NoE2E -NoTauriBuild
#>
[CmdletBinding()]
param(
    [switch]$NoFrontend,
    [switch]$NoE2E,
    [switch]$NoInstall,
    [switch]$NoTauriBuild,
    [switch]$CI
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$LinuxCrates = @('-p', 'stem-core', '-p', 'stem-platform', '-p', 'stem-service')

function Step([string]$Title) {
    Write-Host ''
    Write-Host "==> $Title" -ForegroundColor Cyan
}

# Runs a native command and fails the script on a non-zero exit code
# ($ErrorActionPreference does not cover native commands in PowerShell 5.1).
# Arguments are passed as an array so that `--` reaches the command verbatim.
function Run([string]$Exe, [string[]]$Arguments) {
    Write-Host "+ $Exe $($Arguments -join ' ')" -ForegroundColor DarkGray
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$Exe $($Arguments -join ' ')' failed with exit code $LASTEXITCODE"
    }
}

Step '1/7 rustfmt'
Run cargo @('fmt', '--all', '--check')

Step '2/7 clippy (host target: core, platform, service)'
Run cargo (@('clippy') + $LinuxCrates + @('--all-targets', '--', '-D', 'warnings'))

Step '3/7 tests (host target)'
Run cargo (@('test') + $LinuxCrates)

Step '3b/7 TypeScript bindings are up to date'
# The `ts` feature's tests export the bindings into src/lib/ipc/generated/
# (TS_RS_EXPORT_DIR in .cargo/config.toml). The files are tracked, so any
# modified, added or deleted file after the export means stale bindings.
Run cargo @('test', '-p', 'stem-core', '--features', 'ts', '--quiet')
$changed = @(git status --porcelain src/lib/ipc/generated)
if ($LASTEXITCODE -ne 0) {
    throw "'git status' failed with exit code $LASTEXITCODE"
}
if ($changed.Count -gt 0) {
    Write-Host "src/lib/ipc/generated/ changed after export; run 'cargo test -p stem-core --features ts' and commit the result:" -ForegroundColor Red
    git --no-pager status --short src/lib/ipc/generated
    git --no-pager diff src/lib/ipc/generated
    exit 1
}

Step '3c/7 settings reference (docs/SETTINGS.md) is up to date'
# crates/stem-core/tests/settings_reference.rs rewrites docs/SETTINGS.md from
# the doc comments and defaults in crates/stem-core/src/settings.rs when
# STEM_UPDATE_SETTINGS_REFERENCE=1 (without it, rung 3 already fails on a
# stale page). The file is tracked, so a change after the rewrite means it was
# stale or never committed.
$env:STEM_UPDATE_SETTINGS_REFERENCE = '1'
try {
    Run cargo @('test', '-p', 'stem-core', '--test', 'settings_reference', '--quiet')
}
finally {
    Remove-Item Env:STEM_UPDATE_SETTINGS_REFERENCE -ErrorAction SilentlyContinue
}
$changed = @(git status --porcelain docs/SETTINGS.md)
if ($LASTEXITCODE -ne 0) {
    throw "'git status' failed with exit code $LASTEXITCODE"
}
if ($changed.Count -gt 0) {
    Write-Host "docs/SETTINGS.md changed after regeneration; run 'STEM_UPDATE_SETTINGS_REFERENCE=1 cargo test -p stem-core --test settings_reference' and commit the result:" -ForegroundColor Red
    git --no-pager status --short docs/SETTINGS.md
    git --no-pager diff docs/SETTINGS.md
    exit 1
}

Step '4/7 clippy (Windows target, whole workspace incl. Tauri app)'
Run cargo @('clippy', '--workspace', '--all-targets', '--target', 'x86_64-pc-windows-msvc', '--', '-D', 'warnings')
# The diagnostic probes (feature `probes`) are not in the line above:
# their examples need the feature (`required-features`), so lint them here.
Run cargo @('clippy', '-p', 'stem-platform', '--all-targets', '--features', 'probes', '--target', 'x86_64-pc-windows-msvc', '--', '-D', 'warnings')
# Their unit tests (module windows::probe) run only on a Windows host: the
# module is Windows-only, and rung 3 runs without the feature.
if ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT) {
    Run cargo @('test', '-p', 'stem-platform', '--features', 'probes', '--lib', 'windows::probe::')
}
else {
    Write-Host '(probe unit tests skipped: they need a Windows host)'
}

$Install = -not $NoInstall
if ($CI -and (Test-Path -LiteralPath 'node_modules')) {
    $Install = $false
}

if ($NoFrontend) {
    Write-Host '(frontend rungs 5-7 and the Tauri build skipped: -NoFrontend)'
}
elseif (-not (Test-Path -LiteralPath 'package.json')) {
    if ($CI) {
        throw 'package.json not found; frontend rungs 5-7 cannot run'
    }
    Write-Host '(frontend rungs 5-7 and the Tauri build skipped: no package.json yet)'
}
else {
    Step '5/7 frontend: install, check, lint, test'
    if ($Install) {
        Run pnpm @('install', '--frozen-lockfile')
    }
    else {
        Write-Host '(pnpm install skipped)'
    }
    Run pnpm @('check')
    Run pnpm @('lint')
    Run pnpm @('test')

    Step '6/7 frontend: production build'
    Run pnpm @('build')

    if ($NoE2E) {
        Write-Host '(rung 7 skipped: -NoE2E)'
    }
    else {
        Step '7/7 frontend: Playwright screenshots (mock mode)'
        Run pnpm @('e2e:screenshots')
    }

    if ($NoTauriBuild) {
        Write-Host '(Tauri build skipped: -NoTauriBuild)'
    }
    else {
        # Runs `pnpm build` again via beforeBuildCommand, then a debug cargo
        # build and the NSIS bundler; output under target/debug/.
        Step '8/8 tauri build --debug (links and bundles the real app)'
        Run pnpm @('tauri', 'build', '--debug')
    }
}

Write-Host ''
Write-Host 'All checks passed.' -ForegroundColor Green
