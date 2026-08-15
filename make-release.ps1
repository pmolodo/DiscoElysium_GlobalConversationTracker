#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Builds the plugin and packages it into a versioned, shippable .zip.

.DESCRIPTION
    Runs the Release build, reads the version out of the csproj, and writes

        .build\dist\UnifiedConversationTracker-v<version>.zip

    laid out so that extracting it into a Disco Elysium game folder installs the
    plugin:

        BepInEx\plugins\UnifiedConversationTracker\UnifiedConversationTracker*.dll
        UnifiedConversationTracker-README.md

    That is the plugin plus the mod's own library assemblies (Core, Persistence,
    Session), which BepInEx resolves out of the plugin's own folder.

    BepInEx itself is deliberately NOT bundled. This plugin needs BepInEx 6
    (IL2CPP / CoreCLR) bleeding-edge builds, whose interop assemblies have to be
    generated on the player's own machine from their copy of the game, so an
    "all in one" archive could not work the way a Mono game's would. The README
    points at the BepInEx install instructions instead.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# Build helpers + shared project config, transitively including provision-refs.ps1.
. (Join-Path $PSScriptRoot "build.ps1")

# Shipped alongside the DLL, renamed so it is obvious which mod it documents
# once it has been extracted into the game folder next to disco.exe.
$ReadmeSource = Join-Path $ProjectDir "README.md"
$ReadmeReleaseName = "$AssemblyName-README.md"


Invoke-ScriptMain {

# --- Build --------------------------------------------------------------------
$dllPath = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir
$version = Get-PluginVersion

# --- Stage --------------------------------------------------------------------
# Staged as the exact tree the zip should contain, so the archive extracts
# straight into a game folder.
$stageDir = Join-Path $BuildDir "stage"
if (Test-Path -LiteralPath $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null

Write-Host "Staging release contents:"
Copy-PluginPayload -DllPath $dllPath -DestDir (Get-PluginInstallDir -GameDir $stageDir)

if (-not (Test-Path -LiteralPath $ReadmeSource)) {
    throw "Missing plugin README: $ReadmeSource"
}
Copy-Item -LiteralPath $ReadmeSource -Destination (Join-Path $stageDir $ReadmeReleaseName)
Write-Host "  $ReadmeReleaseName"

# --- Zip ----------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$zipPath = Join-Path $DistDir "$AssemblyName-v$version.zip"
if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}
$contents = Get-ChildItem -Force -LiteralPath $stageDir | ForEach-Object { $_.FullName }
Compress-Archive -Path $contents -DestinationPath $zipPath
Remove-Item -LiteralPath $stageDir -Recurse -Force

Write-Host ""
Write-Host "Created release: $zipPath" -ForegroundColor Green
Write-Host "Install by extracting it into the game folder (the one with $GameExeName)."

}
