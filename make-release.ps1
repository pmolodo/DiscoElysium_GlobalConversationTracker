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

        BepInEx\plugins\GlobalConversationTracker\GlobalConversationTracker*
        BepInEx\plugins\GlobalConversationTracker\Google.Protobuf.dll
        GlobalConversationTracker-README.md

    The mod's own layers (Core, Persistence, Session, Engine) are compiled into
    the plugin assembly, so they are one DLL, and the .pdb is left out. Beside it
    travel the look-ahead engine, the conversation index and the variable table -
    all named GlobalConversationTracker-something so that nothing here has to be
    told they exist - and Google.Protobuf, which cannot be
    renamed because .NET resolves an assembly by its identity rather than by its
    filename. What is staged comes from Get-PluginPayloadFile either way.

    This archive assumes a working BepInEx 6 IL2CPP install already exists -
    which is every developer, and almost no player.

    Then, unless -PluginOnly says otherwise, a second archive for the players who
    do not:

        .build\dist\GlobalConversationTracker-v<version>-AllInOne.zip

    which carries a pinned BepInEx 6 IL2CPP build alongside the plugin, the
    licence that redistributing BepInEx requires, and an uninstaller that
    reverses the whole install. The BepInEx archive is downloaded once per
    machine, verified against a pinned SHA256, and cached; see the BepInEx
    section at the top of build-support.psm1 for what is pinned and why.

    What the bundle cannot ship is the IL2CPP interop assemblies under
    BepInEx\interop: they are generated from the player's own copy of the game on
    first launch. That is why the first launch after installing is a slow one.

.PARAMETER NativeProfile
    The cargo profile the look-ahead engine and the state library are built
    with, and the folder under target\ the C# build then copies them from.
    Defaults to release, because this is what goes out: the profile the measurements
    are taken with and the numbers describe.
.PARAMETER Configuration
    The MSBuild configuration built and then packaged; "Release" unless given.
    The archive is named from the csproj's <Version> alone, so a zip built from
    another configuration is indistinguishable by its file name.

.PARAMETER DiscoElysiumDir
    Game install to read the build's reference assemblies from, overriding
    provision-refs.ps1's usual resolution order (DEGCT_GAME_DIR, the
    repo-local reference copy, the cached previous answer, Steam discovery). It
    must already have BepInEx\core and BepInEx\interop; a path that does not is
    an error rather than a reason to fall back. Nothing from that install is
    packaged: the game and BepInEx assemblies are referenced with
    Private="false".

.PARAMETER PluginOnly
    Skip the all-in-one archive and emit only the plugin zip. Useful when
    iterating on packaging, or on a machine that cannot reach
    builds.bepinex.dev; the BepInEx download is cached per machine, so this
    avoids a one-off 34 MB rather than a per-release cost.

.PARAMETER BundleOnly
    The other way round: emit only the all-in-one archive.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    # Which cargo profile the native artifacts are built with - see
    # Invoke-NativeBuild in build-support.psm1.
    [string]$NativeProfile = "release",
    [string]$DiscoElysiumDir,
    [switch]$PluginOnly,
    [switch]$BundleOnly
)

$ErrorActionPreference = "Stop"

# A module, not a dot-sourced script, so its names cannot land in this scope and
# overwrite the parameters above. -DisableNameChecking: Assert-NotReferenceCopy
# uses a verb that is not on PowerShell's approved list.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

# Shipped alongside the DLL, renamed so it is obvious which mod it documents
# once it has been extracted into the game folder next to disco.exe.
$ReadmeSource = Join-Path $ProjectDir "README.md"
$ReadmeReleaseName = "$AssemblyName-README.md"


Invoke-ScriptMain {

# --- Build --------------------------------------------------------------------
$dllPath = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir -NativeProfile $NativeProfile
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

# The licences travel with the DLLs. Ours because MIT asks the notice to travel
# with the software, and Google.Protobuf's because this archive redistributes it
# in binary form and BSD-3-Clause asks the same - its LICENSE carries the
# copyright notice and the conditions, which is what has to be here.
#
# That is the whole of the third party in this archive. The all-in-one bundle
# carries BepInEx as well and has a notice naming all three; here two licence
# files say it without prose between them.
Copy-PluginLicense -StageDir $stageDir
Copy-Item -LiteralPath (Get-GoogleProtobufLicense) `
    -Destination (Join-Path $stageDir $GoogleProtobufLicenseReleaseName)
Write-Host "  $GoogleProtobufLicenseReleaseName"

# --- Zip ----------------------------------------------------------------------
# Both file operations go through Invoke-WithFileRetry: replacing an archive
# written moments ago is exactly when a virus scanner or Explorer preview still
# has it open.
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
