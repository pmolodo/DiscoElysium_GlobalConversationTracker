#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
<#
.SYNOPSIS
    Builds the plugin and installs it into a playable copy of Disco Elysium.

.DESCRIPTION
    The everyday iterate command: edit -> .\deploy.ps1 -> relaunch the game.

      1. Builds UnifiedConversationTracker.dll (via build.ps1).
      2. Works out which game folder to install into, and says so out loud
         before touching anything.
      3. Deletes any previous <game>\BepInEx\plugins\UnifiedConversationTracker
         folder and copies the fresh build in.
      4. Prints where to look for the plugin's log line.

    The install target comes from, in order:

      1. -GameDir <path>
      2. the DISCO_ELYSIUM_DEPLOY_DIR environment variable
      3. the auto-discovered Steam copy (the everyday case, no flag needed)

    Only when all three come up empty - no override given and no Steam install
    found - does the script stop and ask you to name a target.

    Defaulting to the Steam copy is not the thing keeping you safe; the guards
    are. The resolved target is printed before anything is written, the repo's
    "Steam Install - *" reference copy is refused outright (see de-omm.13,
    -AllowReferenceCopy overrides), a copy without BepInEx is rejected because
    the plugin could never load there, and the only directory ever created or
    deleted is <game>\BepInEx\plugins\UnifiedConversationTracker.
#>
[CmdletBinding()]
param(
    [string]$GameDir,
    [string]$Configuration = "Release",
    # Install to build against; unrelated to -GameDir, which is written to.
    [string]$DiscoElysiumDir,
    [switch]$AllowReferenceCopy,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# Build helpers + shared project config, transitively including
# provision-refs.ps1 (Find-SteamGameDir, Get-PluginInstallDir, ...).
. (Join-Path $PSScriptRoot "build.ps1")


Invoke-ScriptMain {

# --- 1. Resolve and vet the target -------------------------------------------
# Before the build, so an unusable target fails in a second rather than after a
# full compile. Resolve-TargetGameDir (provision-refs.ps1) is the same
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
# Say plainly what is about to be written, before writing it.
Write-Host ""
Write-Host "== Installing ==" -ForegroundColor Cyan
Write-Host "About to write into:" -ForegroundColor Yellow
Write-Host "  $pluginDir" -ForegroundColor Yellow
if (Test-Path -LiteralPath $pluginDir) {
    Write-Host "  (existing install found; it will be replaced)"
}
if ($DryRun) {
    Write-Host "-DryRun given; nothing written." -ForegroundColor Green
    return
}

if (Test-Path -LiteralPath $pluginDir) {
    Remove-Item -LiteralPath $pluginDir -Recurse -Force
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
