#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Resolves and verifies the Disco Elysium install that supplies the build's
    reference assemblies.

.DESCRIPTION
    GlobalConversationTracker.Plugin.csproj references DLLs in place, out of a
    Disco Elysium install that already has BepInEx 6 (IL2CPP) set up:

      * <game>\BepInEx\core     - BepInEx / Il2CppInterop / Harmony assemblies
      * <game>\BepInEx\interop  - IL2CPP interop assemblies for the game's own
                                  code (Assembly-CSharp, DialogueSystem, ...)

    Unlike a Mono game, none of these can be downloaded: the interop assemblies
    are generated on the machine by running the game once with BepInEx
    installed, and they are derived from the game's proprietary GameAssembly.dll.
    So there is nothing to fetch - "provisioning" here means locating an install
    that already has them and proving every DLL the csproj wants is present.

    Resolution order for that reference install:

      1. -DiscoElysiumDir
      2. the DISCO_ELYSIUM_DIR environment variable
      3. <repo>\Steam Install - Unaltered\Disco Elysium (untracked, so absent
         in a git worktree)
      4. the last resolved install, cached per-machine in
         %LOCALAPPDATA%\GlobalConversationTracker\reference-game-dir.txt
      5. Steam auto-discovery (registry + libraryfolders.vdf, AppID 632470)

    Steps 1 to 4 are the same four Directory.Build.props applies to the three
    projects that reference a game install, so 'dotnet build' at the repo root
    resolves exactly as this script does, without running it. Only step 5,
    Steam discovery, needs PowerShell - which is why the cache exists: whichever
    route answers the question, the answer is written there for every checkout
    on the machine to read, worktrees included.

    The reference install is only ever READ from; nothing is written into it.

    The work itself lives in build-support.psm1 (Resolve-ReferenceGameDir,
    Initialize-BuildReferences, Find-SteamGameDir, Assert-NotReferenceCopy, ...);
    import that module to reuse any of it, and run this script to just resolve
    and verify.

.PARAMETER DiscoElysiumDir
    Game install to take the reference assemblies from, skipping the other three
    steps of the resolution order above. Unlike those, it is never silently
    rejected: a path without BepInEx\core and BepInEx\interop throws instead of
    falling through to the next candidate, on the grounds that an explicitly
    named install that cannot be used is a mistake worth reporting. It is
    cached like every other route that resolves, so naming an install once is
    enough for later builds in any checkout on the machine. Leaving it off is
    what asks for the rest of the resolution order, ending in the
    auto-discovered Steam copy.
#>
[CmdletBinding()]
param(
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# Shared project config and functions. A module, not a dot-sourced script, so
# that its own names cannot land in this script's scope and overwrite the
# parameter above - see the header of build-support.psm1.
# -DisableNameChecking: Assert-NotReferenceCopy uses a verb PowerShell does not
# have on its approved list, and the name says what it does better than any
# approved verb would.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

Invoke-ScriptMain {
    Initialize-BuildReferences -DiscoElysiumDir $DiscoElysiumDir | Out-Null
}
