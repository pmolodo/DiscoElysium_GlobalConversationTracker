#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Builds the plugin and installs it into a playable copy of Disco Elysium.

.DESCRIPTION
    The everyday iterate command: edit -> .\deploy.ps1 -> relaunch the game.

      1. Builds UnifiedConversationTracker.dll (via build.ps1).
      2. Works out which game folder to install into, and says so out loud
         before touching anything.
      3. Clears the previous build out of
         <game>\BepInEx\plugins\UnifiedConversationTracker and copies the fresh
         one in. Only the plugin's own UnifiedConversationTracker*.dll/.pdb are
         removed, so anything else kept in that folder - notably the optional
         articy_ids_final_cut.json - survives a redeploy.
      4. Prints where to look for the plugin's log line.

    The install target comes from, in order:

      1. -GameDir <path>
      2. the DISCO_ELYSIUM_DEPLOY_DIR environment variable
      3. the auto-discovered Steam copy (the everyday case, no flag needed)

    Only when all three come up empty - no override given and no Steam install
    found - does the script stop and ask you to name a target.

    Defaulting to the Steam copy is not the thing keeping you safe; the guards
    are. The resolved target is printed before anything is written, the repo's
    "Steam Install - *" reference copy is refused outright (-AllowReferenceCopy
    overrides), a copy without BepInEx is rejected because the plugin could
    never load there, the only directory ever created is
    <game>\BepInEx\plugins\UnifiedConversationTracker, and the only files ever
    deleted are that folder's own UnifiedConversationTracker*.dll/.pdb.

.PARAMETER GameDir
    The playable game folder to install into, taking priority over
    DISCO_ELYSIUM_DEPLOY_DIR and over Steam auto-discovery. It has to contain
    disco.exe and a BepInEx\core, and it must not be one of the repo's reference
    copies unless -AllowReferenceCopy says otherwise. Whatever it resolves to is
    printed before anything is written.

.PARAMETER Configuration
    The configuration built before installing; "Release" unless given. The
    installed files are that build's output, so this decides which build ends up
    in the game folder as well as which one is compiled.

.PARAMETER DiscoElysiumDir
    Game install to compile against - unrelated to -GameDir, which is the
    install written into. The two are separate because the reference assemblies
    can legitimately come from a copy you would never deploy to, such as the
    repo's read-only reference copy. A path without BepInEx\core and
    BepInEx\interop is an error rather than a reason to fall back; leaving it off
    is what asks for the usual resolution order (see provision-refs.ps1), ending
    in the auto-discovered Steam copy.

.PARAMETER AllowReferenceCopy
    Deploy even when the target turns out to be one of the repo's read-only
    reference copies of the game ("Steam Install - Unaltered"), which is
    otherwise refused outright. The deploy then goes ahead with a warning, so
    testing against a reference copy stays possible but never happens by
    accident.

.PARAMETER DryRun
    Stop after resolving the target and building, printing the plugin folder
    whose payload would have been replaced without deleting or copying anything.
    The build still runs, so this checks the whole path up to the write.
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

# Build helpers + shared project config (Invoke-PluginBuild, Find-SteamGameDir,
# Get-PluginInstallDir, ...). A module, not a dot-sourced script, so that its
# own names cannot land in this script's scope and overwrite the parameters
# above - see the header of build-support.psm1 (de-3pw).
# -DisableNameChecking: Assert-NotReferenceCopy uses a verb PowerShell does not
# have on its approved list, and the name says what it does better than any
# approved verb would.
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
# Say plainly what is about to be written, before writing it. Only this plugin's
# own files are replaced; anything else in the folder is left where it is
# (de-bx9), which is what lets a hand-placed articy_ids_final_cut.json stay put.
Write-Host ""
Write-Host "== Installing ==" -ForegroundColor Cyan
Write-Host "About to write into:" -ForegroundColor Yellow
Write-Host "  $pluginDir" -ForegroundColor Yellow
if (Test-Path -LiteralPath $pluginDir) {
    Write-Host "  (existing install found; its $AssemblyName files will be replaced, anything else there left alone)"
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
