#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

<#
.SYNOPSIS
    Builds the plugin and packages it into a versioned, shippable .zip.

.DESCRIPTION
    Runs the Release build, reads the version out of the csproj, and writes

        .build\dist\GlobalConversationTracker-v<version>.zip

    laid out so that extracting it into a Disco Elysium game folder installs the
    plugin:

        BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker*.dll
        GlobalConversationTracker-README.md

    That is one DLL: the mod's own layers (Core, Persistence, Session) are
    compiled into the plugin assembly, so nothing has to be installed beside it,
    and the .pdb is left out of the archive because debugging symbols are of no
    use in a player's install. This archive assumes the player already has a working
    BepInEx 6 IL2CPP install - which is every developer, and almost no player.

    Then, unless -PluginOnly says otherwise, a second archive for the players who
    do not:

        .build\dist\GlobalConversationTracker-v<version>-AllInOne.zip

    which carries a pinned BepInEx 6 IL2CPP build alongside the plugin, the
    licence that redistributing BepInEx requires, and an uninstaller that
    reverses the whole install. The BepInEx archive is downloaded once per
    machine, verified against a pinned SHA256, and cached; see the BepInEx
    section at the top of build-support.psm1 for what is pinned and why.

    What the bundle cannot ship is the IL2CPP interop assemblies under
    BepInEx\interop: they are generated from the player's own copy of the game
    on first launch and are specific to that build. That is not a gap - BepInEx
    writes them the first time the game runs, which is why the first launch
    after installing is a slow one.

.PARAMETER Configuration
    The MSBuild configuration that gets built and then packaged; "Release"
    unless given, and there is rarely a reason to ship anything else. Note that
    the archive is named from the csproj's <Version> alone, so a zip built from
    another configuration is indistinguishable by its file name.

.PARAMETER DiscoElysiumDir
    Game install to read the build's reference assemblies from, overriding
    provision-refs.ps1's usual resolution order (DISCO_ELYSIUM_DIR, the
    repo-local reference copy, the cached previous answer, Steam discovery). It
    must already have BepInEx\core and BepInEx\interop, and a path that does not
    is an error rather than a reason to fall back. Leaving it off is what asks
    for that resolution order, ending in the auto-discovered Steam copy. It
    affects only what the
    build compiles against and never appears in the archive: the game and
    BepInEx assemblies are referenced with Private="false", so nothing from that
    install is packaged.
.PARAMETER PluginOnly
    Skip the all-in-one archive and emit only the plugin zip. Useful when
    iterating on packaging, or on a machine that cannot reach
    builds.bepinex.dev; the BepInEx download is cached per machine, so the cost
    it avoids is a one-off 34 MB rather than a per-release one.

.PARAMETER BundleOnly
    The other way round: emit only the all-in-one archive.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir,
    [switch]$PluginOnly,
    [switch]$BundleOnly
)

$ErrorActionPreference = "Stop"

# Build helpers + shared project config (Invoke-PluginBuild, Copy-PluginPayload,
# Get-PluginVersion, ...). A module, not a dot-sourced script, so that its own
# names cannot land in this script's scope and overwrite the parameters above -
# see the header of build-support.psm1.
# -DisableNameChecking: Assert-NotReferenceCopy uses a verb PowerShell does not
# have on its approved list, and the name says what it does better than any
# approved verb would.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

# Shipped alongside the DLL, renamed so it is obvious which mod it documents
# once it has been extracted into the game folder next to disco.exe.
$ReadmeSource = Join-Path $ProjectDir "README.md"
$ReadmeReleaseName = "$AssemblyName-README.md"


Invoke-ScriptMain {

# --- Build --------------------------------------------------------------------
$dllPath = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir
$version = Get-PluginVersion

if ($PluginOnly -and $BundleOnly) {
    throw "-PluginOnly and -BundleOnly are opposites; give one or neither."
}

$made = [System.Collections.Generic.List[string]]::new()

# --- The plugin-only archive ---------------------------------------------------
# Staged as the exact tree the zip should contain, so the archive extracts
# straight into a game folder.
if (-not $BundleOnly) {
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

# The licence travels with the DLL. This archive bundles nothing of anyone
# else's, so it needs no third-party notice - only our own terms.
Copy-PluginLicense -StageDir $stageDir

# --- Zip ----------------------------------------------------------------------
# Both file operations go through Invoke-WithFileRetry: replacing an archive
# that was written moments ago is exactly when a virus scanner or an Explorer
# preview still has it open, and that is worth a retry and a sentence rather
# than a raw .NET lock error at the end of an otherwise successful run.
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$zipPath = Join-Path $DistDir "$AssemblyName-v$version.zip"
try {
    if (Test-Path -LiteralPath $zipPath) {
        Invoke-WithFileRetry -Path $zipPath -What "replace" -Operation {
            Remove-Item -LiteralPath $zipPath -Force -ErrorAction Stop
        }
    }
    $contents = Get-ChildItem -Force -LiteralPath $stageDir | ForEach-Object { $_.FullName }
    Invoke-WithFileRetry -Path $zipPath -What "write" -Operation {
        Compress-Archive -Path $contents -DestinationPath $zipPath -ErrorAction Stop
    }
}
finally {
    # Cleared even when the zip could not be written, so a failed run leaves no
    # half-finished staging folder for the next one to trip over.
    Remove-Item -LiteralPath $stageDir -Recurse -Force -ErrorAction SilentlyContinue
}
$made.Add($zipPath)
}

# --- The all-in-one bundle ----------------------------------------------------
if (-not $PluginOnly) {
    Write-Host ""
    $bundlePath = New-AllInOneBundle `
        -DllPath $dllPath `
        -Version $version `
        -ZipPath (Join-Path $DistDir "$AssemblyName-v$version-$BundleSuffix.zip") `
        -StageDir (Join-Path $BuildDir "stage-$BundleSuffix") `
        -ReadmeSource $ReadmeSource `
        -ReadmeName $ReadmeReleaseName
    $made.Add($bundlePath)
}

Write-Host ""
foreach ($path in $made) {
    Write-Host "Created release: $path" -ForegroundColor Green
}
Write-Host "Install by extracting into the game folder (the one with $GameExeName)."
if (-not $PluginOnly) {
    Write-Host "The $BundleSuffix archive brings BepInEx $BepInExVersion with it and can be undone with the $UninstallerName it installs."
}

}
