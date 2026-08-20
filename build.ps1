#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Builds UnifiedConversationTracker.dll, the BepInEx plugin.

.DESCRIPTION
    Resolves and verifies the reference assemblies (the same resolution
    provision-refs.ps1 runs), then runs `dotnet build` on
    src\UnifiedConversationTracker.Plugin, handing the resolved game directory
    to the csproj as -p:DiscoElysiumDir so the project's own
    reference-resolution mechanism is what actually runs.

    Nothing is written outside the repo: build output lands in the gitignored
    .build\ folder (see Directory.Build.props). The game install is only read
    from.

    The work itself lives in build-support.psm1 (Invoke-PluginBuild,
    Copy-PluginPayload, Get-PluginVersion, and the reference resolution beneath
    them); import that module to reuse any of it, and run this script to just
    build.

.PARAMETER Configuration
    The MSBuild configuration handed to `dotnet build -c`; "Release" unless
    given. It also selects which output directory the built DLL is picked up
    from (.build\bin\<project>\<Configuration>\<tfm>\), so a caller that goes on
    to deploy or package the result ships the configuration it asked for.

.PARAMETER DiscoElysiumDir
    Game install to read the build's reference assemblies from, taking priority
    over the rest of provision-refs.ps1's resolution order (DISCO_ELYSIUM_DIR,
    the repo-local reference copy, the cached previous answer, Steam discovery).
    It has to be an install that has already been run once with BepInEx 6, since
    BepInEx\interop is generated on the machine and cannot be fetched; a path
    without BepInEx\core and BepInEx\interop is an error rather than a reason to
    fall back. Leaving it off is what asks for that resolution order, ending in
    the auto-discovered Steam copy. The install is only read from.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# Shared project config and functions. A module, not a dot-sourced script, so
# that its own names cannot land in this script's scope and overwrite the
# parameters above - see the header of build-support.psm1.
# -DisableNameChecking: Assert-NotReferenceCopy uses a verb PowerShell does not
# have on its approved list, and the name says what it does better than any
# approved verb would.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

Invoke-ScriptMain {
    $dll = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir
    Write-Host "Built: $dll" -ForegroundColor Green
}
