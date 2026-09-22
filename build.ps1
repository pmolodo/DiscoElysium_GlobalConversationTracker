#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

<#
.SYNOPSIS
    Builds GlobalConversationTracker.dll, the BepInEx plugin.

.DESCRIPTION
    Resolves and verifies the reference assemblies (the same resolution
    provision-refs.ps1 runs), then runs `dotnet build` on
    src\GlobalConversationTracker.Plugin, handing the resolved game directory to
    the csproj as -p:DiscoElysiumDir.

    Nothing is written outside the repo: build output lands in the gitignored
    .build\ folder (see Directory.Build.props), and the game install is only read
    from.

    The work lives in build-support.psm1 (Invoke-PluginBuild, Copy-PluginPayload,
    Get-PluginVersion, and the reference resolution beneath them).

.PARAMETER Configuration
    The MSBuild configuration handed to `dotnet build -c`; "Release" unless
    given. It also selects the output directory the built DLL is picked up from
    (.build\bin\<project>\<Configuration>\<tfm>\).

.PARAMETER DiscoElysiumDir
    Game install to read reference assemblies from, taking priority over the rest
    of provision-refs.ps1's resolution order (DEGCT_GAME_DIR, the repo-local
    reference copy, the cached previous answer, Steam discovery). It must be an
    install already run once with BepInEx 6, since BepInEx\interop is generated
    on the machine and cannot be fetched; a path without BepInEx\core and
    BepInEx\interop is an error rather than a reason to fall back.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# A module, not a dot-sourced script, so its names cannot land in this scope and
# overwrite the parameters above. -DisableNameChecking: Assert-NotReferenceCopy
# uses a verb that is not on PowerShell's approved list.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

Invoke-ScriptMain {
    $dll = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir
    Write-Host "Built: $dll" -ForegroundColor Green
}
