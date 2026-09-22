#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

<#
.SYNOPSIS
    Builds the plugin and installs it into a playable copy of Disco Elysium.

.DESCRIPTION
    The everyday iterate command: edit -> .\deploy.ps1 -> relaunch the game.

      1. Builds the look-ahead engine and the state library with cargo, in
         release - the plugin's own build COPIES those rather than building
         them - and then GlobalConversationTracker.dll.
      2. Works out which game folder to install into, and says so out loud
         before touching anything.
      3. Clears the previous build out of
         <game>\BepInEx\plugins\GlobalConversationTracker and copies the fresh
         one in. The whole plugin payload is replaced - the plugin DLL, the
         look-ahead engine executable, the conversation index and the other
         files Get-PluginPayloadFile in build-support.psm1 selects - and every
         file removed is written back from the fresh build. The .pdb is not
         deployed; one left by an older build is left alone. A file outside
         that payload, notably the optional articy_ids_final_cut.json,
         survives.
      4. Prints where to look for the plugin's log line.

    The install target comes from, in order:

      1. -GameDir <path>
      2. the DEGCT_DEPLOY_DIR environment variable
      3. the auto-discovered Steam copy (the everyday case, no flag needed)

    Only when all three come up empty does the script stop and ask for a target.

    The guards, not the default, are what keep this safe: the resolved target is
    printed before anything is written, a repo reference copy is refused outright
    (-AllowReferenceCopy overrides), a copy without BepInEx is rejected, the only
    directory ever created is <game>\BepInEx\plugins\GlobalConversationTracker,
    and the only files ever deleted are that folder's own plugin payload, as
    Get-PluginPayloadFile defines it.

.PARAMETER GameDir
    The playable game folder to install into, taking priority over
    DEGCT_DEPLOY_DIR and Steam auto-discovery. Must contain disco.exe and
    a BepInEx\core, and must not be one of the repo's reference copies unless
    -AllowReferenceCopy says otherwise.

.PARAMETER Configuration
    The configuration built before installing; "Release" unless given. The
    installed files are that build's output.

.PARAMETER DiscoElysiumDir
    Game install to compile against - unrelated to -GameDir, which is the install
    written into. They are separate because the reference assemblies can come
    from a copy you would never deploy to. A path without BepInEx\core and
    BepInEx\interop is an error rather than a reason to fall back; leaving it off
    asks for the usual resolution order (see provision-refs.ps1).

.PARAMETER AllowReferenceCopy
    Deploy even when the target is inside the repo's read-only reference material
    under .game_reference_copies, which is otherwise refused. The deploy goes
    ahead with a warning.

.PARAMETER DryRun
    Stop after resolving the target and building, printing the plugin folder
    whose payload would have been replaced. The build still runs, so this checks
    the whole path up to the write.
#>
[CmdletBinding()]
param(
    [string]$GameDir,
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir,
    [switch]$AllowReferenceCopy,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# A module, not a dot-sourced script, so its names cannot land in this scope and
# overwrite the parameters above. -DisableNameChecking: Assert-NotReferenceCopy
# uses a verb that is not on PowerShell's approved list.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking


Invoke-ScriptMain {

# --- 1. Resolve and vet the target -------------------------------------------
# Before the build, so an unusable target fails in a second rather than after a
# full compile. Resolve-TargetGameDir (build-support.psm1) is the same
# resolution capture-log.ps1 uses, so both act on the same install.
Write-Host "== Resolving deploy target ==" -ForegroundColor Cyan
$gameDir = Resolve-TargetGameDir -GameDir $GameDir

if ($AllowReferenceCopy) {
    if (Test-IsReferenceCopy -Path $gameDir) {
        Write-Warning "Target is a read-only reference copy of the game; -AllowReferenceCopy was given, so writing anyway."
    }
}
else {
    Assert-NotReferenceCopy -Path $gameDir
}

if (-not (Test-Path -LiteralPath (Join-Path $gameDir $BepInExCoreRelDir))) {
    throw "No $BepInExCoreRelDir in $gameDir - BepInEx is not installed in this copy of the game, so the plugin would never load. Install BepInEx 6 (IL2CPP) there first."
}

$pluginDir = Get-PluginInstallDir -GameDir $gameDir

# --- 2. Build -----------------------------------------------------------------
Write-Host ""
Write-Host "== Building ==" -ForegroundColor Cyan
$dllPath = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir

# --- 3. Replace the previous install -----------------------------------------
# Say what is about to be written, before writing it. Only this plugin's own
# files are replaced, which is what lets a hand-placed
# articy_ids_final_cut.json stay put.
Write-Host ""
Write-Host "== Installing ==" -ForegroundColor Cyan
Write-Host "About to write into:" -ForegroundColor Yellow
Write-Host "  $pluginDir" -ForegroundColor Yellow
if (Test-Path -LiteralPath $pluginDir) {
    Write-Host "  (existing install found; its whole plugin payload will be removed and rewritten from this build, anything else there left alone)"
}
if ($DryRun) {
    Write-Host "-DryRun given; nothing written." -ForegroundColor Green
    return
}

$removedCount = Remove-PluginPayload -DestDir $pluginDir
if ($removedCount -gt 0) {
    Write-Host "Removed $removedCount file(s) of the previous install."
}
Copy-PluginPayload -DllPath $dllPath -DestDir $pluginDir

# --- 4. Tell the user how to verify ------------------------------------------
$logPath = Join-Path $gameDir $BepInExLogRelPath
$configPath = Join-Path $gameDir $BepInExConfigRelPath
Write-Host ""
Write-Host "Deployed $AssemblyName v$(Get-PluginVersion). Launch the game to test." -ForegroundColor Green
Write-Host "Log: $logPath"
Write-Host "Look for: [Message:$AssemblyName] $AssemblyName v$(Get-PluginVersion) loaded."

# The in-game console is the fastest way to see that line; report its state
# rather than editing the user's BepInEx config behind their back.
if (Test-Path -LiteralPath $configPath) {
    $cfg = [System.IO.File]::ReadAllText($configPath)
    if (-not [regex]::IsMatch($cfg, '(?ms)^\[Logging\.Console\].*?^Enabled\s*=\s*true')) {
        Write-Host "Tip: set [Logging.Console] Enabled = true in $configPath to get a live console window."
    }
}

}
