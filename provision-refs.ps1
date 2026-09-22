#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

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
      2. the DEGCT_GAME_DIR environment variable
      3. the last resolved install, cached per-machine in
         %LOCALAPPDATA%\GlobalConversationTracker\reference-game-dir.txt
      4. Steam auto-discovery (registry + libraryfolders.vdf, AppID 632470)
      5. a copy under <repo>\.game_reference_copies (untracked, so absent in
         a git worktree)

    Steps 1, 2, 4 and 5 are the sources; step 3 is a cache over them, placed
    where the expense begins - the first two lookups are free, discovery is not.
    Among the sources, the live Steam install beats the repo copy: it is the one
    patched and re-run as the game updates, so its interop assemblies match the
    game actually being played.

    Directory.Build.props applies the same list to the three projects that
    reference a game install, minus step 4, since MSBuild cannot read Steam's
    library folders. There the cache is the only channel by which a discovered
    Steam copy ever arrives - which is why every route that resolves here writes
    it, worktrees included.

    The reference install is only ever READ from; nothing is written into it.

    The work lives in build-support.psm1 (Resolve-ReferenceGameDir,
    Initialize-BuildReferences, Find-SteamGameDir, Assert-NotReferenceCopy, ...).

.PARAMETER DiscoElysiumDir
    Game install to take the reference assemblies from, skipping the rest of the
    resolution order above. Unlike those, it is never silently rejected: a path
    without BepInEx\core and BepInEx\interop throws rather than falling through
    to the next candidate. It is cached like every other route that resolves, so
    naming an install once is enough for later builds in any checkout on the
    machine.
#>
[CmdletBinding()]
param(
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# A module, not a dot-sourced script, so its names cannot land in this scope and
# overwrite the parameter above. -DisableNameChecking: Assert-NotReferenceCopy
# uses a verb that is not on PowerShell's approved list.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

Invoke-ScriptMain {
    Initialize-BuildReferences -DiscoElysiumDir $DiscoElysiumDir | Out-Null
}
