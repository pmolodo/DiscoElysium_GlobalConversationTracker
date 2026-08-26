#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Removes everything the GlobalConversationTracker all-in-one bundle installed.

.DESCRIPTION
    Ships inside the all-in-one archive and is extracted next to disco.exe,
    beside the manifest that lists what the archive contained. Run it from the
    game folder to put that folder back the way it was:

        .\Uninstall-GlobalConversationTracker.ps1

    It deletes only files the bundle itself wrote, and only while they still
    hold the bytes the bundle wrote. A file that has changed since - BepInEx
    updated in place, a config edited, another mod's file that happens to share
    a path - is left alone and reported. That is the whole safety property: this
    cannot remove anything it did not install, and it cannot remove a file
    somebody has since made their own.

    What it does NOT remove, deliberately:

      * The global state file in your SaveGames folder. That is your dialogue
        history across every playthrough, it is the entire point of the mod, and
        deleting it would be unrecoverable. -RemoveGlobalState if you mean it.
      * BepInEx's generated data - the interop assemblies, the cache, the logs.
        They are large, they are rebuilt on demand, and they are not ours.
        -RemoveBepInExData to sweep them up as well.
      * Anything under BepInEx\plugins other than this mod's own folder, so a
        second mod installed after this one survives.

    Nothing is written anywhere outside the game folder, and -WhatIf shows the
    whole run without touching a thing.

.PARAMETER GameDir
    The game folder to clean. Defaults to the folder this script is in, which is
    where the archive puts it; give it explicitly if you moved the script.

.PARAMETER RemoveGlobalState
    Also delete the mod's global state file, the record of which dialogue you
    have reached across all saves. Off by default; there is no undo.

.PARAMETER RemoveBepInExData
    Also delete BepInEx's generated data: the IL2CPP interop assemblies, its
    cache, its log, and the preloader logs in the game folder. Off by default,
    because none of it was installed by this bundle - it appeared when you first
    launched the game with BepInEx.

.PARAMETER Keep
    Leave the uninstaller and its manifest behind. By default they delete
    themselves last, since a folder with nothing left to uninstall should not
    keep an uninstaller in it.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$GameDir,
    [switch]$RemoveGlobalState,
    [switch]$RemoveBepInExData,
    [switch]$Keep
)

$ErrorActionPreference = "Stop"

$ManifestName = "GlobalConversationTracker-install-manifest.json"
$GameExeName = "disco.exe"
$PluginGuid = "com.molodowitch.globalconversationtracker"
$StateFileName = "global-conversation-state.json"
# Where the game keeps its saves, and so where the mod keeps its state.
$SaveGamesRelPath = "AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames"
# BepInEx's own generated data, relative to the game folder.
$BepInExDataRelPaths = @("BepInEx\interop", "BepInEx\cache", "BepInEx\LogOutput.log", "BepInEx\ErrorLog.log")


function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}


try {
    if (-not $GameDir) { $GameDir = $PSScriptRoot }
    $GameDir = (Get-Item -LiteralPath $GameDir).FullName

    if (-not (Test-Path -LiteralPath (Join-Path $GameDir $GameExeName))) {
        throw @"
$GameDir does not look like a Disco Elysium install - no $GameExeName in it.
Run this from the game folder, or pass -GameDir "<path to the folder with $GameExeName>".
"@
    }

    $manifestPath = Join-Path $PSScriptRoot $ManifestName
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        throw @"
No $ManifestName next to this script.
That file lists what the bundle installed, and without it this uninstaller has
no way to know what is safe to delete - it will not guess. Re-extract the
archive over the game folder and run this again.
"@
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json

    Write-Host "Uninstalling $($manifest.bundle) v$($manifest.bundleVersion) from $GameDir"
    Write-Host "  (bundled BepInEx $($manifest.bepInEx.version))"
    Write-Host ""

    # Counted apart, because -WhatIf makes ShouldProcess return false for every
    # item: without this the summary would report a couple of hundred files
    # "removed" by a run that deliberately removed nothing.
    $matched = 0
    $removed = 0
    $changed = [System.Collections.Generic.List[string]]::new()
    $missing = 0

    foreach ($entry in $manifest.files) {
        $path = Join-Path $GameDir $entry.path
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $missing++
            continue
        }
        if ((Get-Sha256 -Path $path) -ne $entry.sha256) {
            $changed.Add($entry.path)
            continue
        }
        $matched++
        if ($PSCmdlet.ShouldProcess($path, "Remove file")) {
            Remove-Item -LiteralPath $path -Force
            $removed++
        }
    }

    # Our own config file: written by BepInEx from the mod's own settings, named
    # after the mod's GUID, so there is no doubt whose it is. Its contents are
    # the player's, so it is not hash-checked - it is meant to have been edited.
    $configPath = Join-Path $GameDir "BepInEx\config\$PluginGuid.cfg"
    if (Test-Path -LiteralPath $configPath) {
        $matched++
        if ($PSCmdlet.ShouldProcess($configPath, "Remove file")) {
            Remove-Item -LiteralPath $configPath -Force
            $removed++
        }
    }

    # Deepest first, so a directory empties before its parent is considered.
    $dirs = @($manifest.directories) | Sort-Object -Property Length -Descending
    $prunedDirs = 0
    foreach ($rel in $dirs) {
        $dir = Join-Path $GameDir $rel
        if (-not (Test-Path -LiteralPath $dir -PathType Container)) { continue }
        if (@(Get-ChildItem -LiteralPath $dir -Force).Count -ne 0) { continue }
        if ($PSCmdlet.ShouldProcess($dir, "Remove empty directory")) {
            Remove-Item -LiteralPath $dir -Force
            $prunedDirs++
        }
    }

    if ($RemoveBepInExData) {
        foreach ($rel in $BepInExDataRelPaths) {
            $path = Join-Path $GameDir $rel
            if (-not (Test-Path -LiteralPath $path)) { continue }
            if ($PSCmdlet.ShouldProcess($path, "Remove BepInEx generated data")) {
                Remove-Item -LiteralPath $path -Recurse -Force
            }
            Write-Host "  removed generated: $rel"
        }
        foreach ($log in @(Get-ChildItem -LiteralPath $GameDir -Filter "preloader_*.log" -File -ErrorAction SilentlyContinue)) {
            if ($PSCmdlet.ShouldProcess($log.FullName, "Remove BepInEx preloader log")) {
                Remove-Item -LiteralPath $log.FullName -Force
            }
            Write-Host "  removed generated: $($log.Name)"
        }
    }

    $statePath = Join-Path $env:USERPROFILE (Join-Path $SaveGamesRelPath $StateFileName)
    $stateNote = if (-not (Test-Path -LiteralPath $statePath)) {
        "none found at $statePath"
    }
    elseif ($RemoveGlobalState) {
        if ($PSCmdlet.ShouldProcess($statePath, "Remove global state")) {
            Remove-Item -LiteralPath $statePath -Force
        }
        "removed on request"
    }
    else {
        "kept at $statePath - pass -RemoveGlobalState to delete it"
    }

    Write-Host ""
    if ($WhatIfPreference) {
        # Folders are only counted when they are actually empty, and nothing was
        # deleted here, so the folder figure is a floor rather than the answer.
        Write-Host "Would remove $matched file(s), and any folder they leave empty. Nothing was changed."
    }
    else {
        Write-Host "Removed $removed file(s) and $prunedDirs empty folder(s)."
    }
    if ($missing -gt 0) {
        Write-Host "$missing bundled file(s) were already gone."
    }
    if ($changed.Count -gt 0) {
        Write-Host ""
        Write-Host "Left alone, because they no longer match what the bundle installed:" -ForegroundColor Yellow
        foreach ($path in $changed) { Write-Host "  $path" }
        Write-Host "Something updated or edited these after the install - another mod, a BepInEx"
        Write-Host "upgrade, or you. Delete them by hand if you are sure they are not wanted."
    }
    Write-Host ""
    Write-Host "Global state: $stateNote"
    if (-not $RemoveBepInExData) {
        Write-Host "BepInEx's generated data (interop, cache, logs) was kept - pass -RemoveBepInExData to sweep it up."
    }

    if (-not $Keep) {
        # Last, and in this order: once the manifest is gone this script has
        # nothing left to work from, so it goes too. Windows lets a running
        # script delete its own file - the interpreter has already read it.
        if ($PSCmdlet.ShouldProcess($manifestPath, "Remove manifest")) {
            Remove-Item -LiteralPath $manifestPath -Force
        }
        $self = $PSCommandPath
        if ($PSCmdlet.ShouldProcess($self, "Remove uninstaller")) {
            Remove-Item -LiteralPath $self -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $self) {
                Write-Host ""
                Write-Host "This script could not delete itself; remove $self by hand."
            }
        }
    }

    Write-Host ""
    Write-Host "Done." -ForegroundColor Green
}
catch {
    Write-Host ""
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}
