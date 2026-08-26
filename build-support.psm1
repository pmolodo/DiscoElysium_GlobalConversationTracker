# SPDX-License-Identifier: MIT
<#
    Shared configuration and helpers for the five .ps1 scripts at the repo root.

    A MODULE, deliberately, and not a dot-sourced .ps1. PowerShell runs a
    dot-sourced script's param() block in the CALLER's scope, so a shared script
    declaring -DiscoElysiumDir or -Configuration silently resets its caller's
    copy of that parameter to the default before the caller ever reads it.
    Import-Module has no such scope leak - a module's contents are never
    executed in the importer's scope - so keeping every shared name in here
    makes that whole class of bug impossible.

    Each root script therefore stays a self-contained entry point: its own
    param() block, this module imported, its own main flow. None of them
    dot-source another, and nothing that a script imports declares parameters.

    Exports every function below plus every constant defined in the
    configuration section (see the Export-ModuleMember call at the bottom).
#>

$ErrorActionPreference = "Stop"

# Snapshot of what is already in module scope: the preference variables
# inherited from the session (plus this name itself, added explicitly since the
# assignment has not completed yet). Export-ModuleMember at the bottom exports
# the difference between this and the final set, so a constant added below is
# exported without anyone maintaining a list - and preference variables, which
# belong to the importing script and which PowerShell refuses to let a module
# overwrite, are never among the exports.
$PreConfigVariableNames = @((Get-Variable -Scope Script).Name) + "PreConfigVariableNames"

# --- Shared project configuration --------------------------------------------
# $PSScriptRoot is this module's folder, i.e. the repo root, which is also where
# every script that imports it lives.
$RepoRoot = $PSScriptRoot
$AssemblyName = "GlobalConversationTracker"
$TargetFramework = "net6.0"
$ProjectDir = Join-Path $RepoRoot "src\$AssemblyName.Plugin"
$ProjectFile = Join-Path $ProjectDir "$AssemblyName.Plugin.csproj"

# Everything generated lives under one gitignored folder: bin\ and obj\ are
# redirected here by Directory.Build.props, and these scripts add stage\ and
# dist\ alongside them.
$BuildDir = Join-Path $RepoRoot ".build"
$DistDir = Join-Path $BuildDir "dist"
# Must match BaseOutputPath in Directory.Build.props.
$BinDir = Join-Path $BuildDir "bin\$AssemblyName.Plugin"

# Caches the resolved reference install, so repeat builds skip Steam discovery.
#
# Per-user and OUTSIDE the repo, which is the point: a git worktree does not
# contain the untracked 'Steam Install - Unaltered' copy, and its own .build\ is
# empty, so a cache kept in the repo could never answer the question there. One
# cache per machine answers it for every checkout on that machine, and
# Directory.Build.props reads this same file, so a bare 'dotnet build' resolves
# an install without going through these scripts at all.
$CacheDir = Join-Path $env:LOCALAPPDATA $AssemblyName
$RefDirCacheFile = Join-Path $CacheDir "reference-game-dir.txt"

# --- The BepInEx build the all-in-one bundle ships -----------------------------

function Read-MSBuildProperties {
    # Every <PropertyGroup> child of an MSBuild file as a hashtable, with
    # $(Name) references expanded - against the other properties first, then
    # against the environment, which is how MSBuild itself resolves them.
    #
    # This exists so the BepInEx pin lives in exactly one file. MSBuild needs
    # those values to reference BepInEx\core, this module needs the same ones to
    # package the same archive into the all-in-one release, and a pinned URL and
    # hash kept in two places are a pin waiting to drift.
    param([Parameter(Mandatory = $true)][string]$Path)

    [xml]$doc = Get-Content -LiteralPath $Path -Raw
    $props = @{}
    foreach ($group in @($doc.Project.PropertyGroup)) {
        foreach ($node in $group.ChildNodes) {
            if ($node.NodeType -ne "Element") { continue }
            $props[$node.Name] = $node.InnerText
        }
    }

    # Repeated passes, because one property is written in terms of another and
    # this file's order is for a human rather than for a resolver. Bounded so a
    # circular reference stops rather than spins.
    for ($pass = 0; $pass -lt 10; $pass++) {
        $changed = $false
        foreach ($name in @($props.Keys)) {
            $expanded = [regex]::Replace($props[$name], '\$\((?<ref>[A-Za-z_][A-Za-z0-9_]*)\)', {
                    param($match)
                    $key = $match.Groups["ref"].Value
                    if ($props.ContainsKey($key)) { return $props[$key] }
                    $fromEnv = [Environment]::GetEnvironmentVariable($key)
                    if ($fromEnv) { return $fromEnv }
                    return $match.Value
                })
            if ($expanded -ne $props[$name]) {
                $props[$name] = $expanded
                $changed = $true
            }
        }
        if (-not $changed) { break }
    }
    return $props
}

# Read once at import, from the file MSBuild reads. Bump the pin there, not here.
$BepInExPin = Read-MSBuildProperties -Path (Join-Path $RepoRoot "BepInEx.props")
$BepInExBuild = $BepInExPin["BepInExBuild"]
$BepInExVersion = $BepInExPin["BepInExVersion"]
$BepInExCommit = $BepInExPin["BepInExCommit"]
$BepInExShortCommit = $BepInExPin["BepInExShortCommit"]
$BepInExZipName = $BepInExPin["BepInExZipName"]
$BepInExZipUrl = $BepInExPin["BepInExZipUrl"]
$BepInExZipSha256 = $BepInExPin["BepInExZipSha256"]
$BepInExLicenseUrl = $BepInExPin["BepInExLicenseUrl"]
# Trailing separators come from MSBuild's habit of ending directory properties
# with one; PowerShell's paths read better without.
$BepInExCacheDir = $BepInExPin["BepInExCacheDir"].TrimEnd("\")
$BepInExUnpackedDir = $BepInExPin["BepInExUnpackedDir"].TrimEnd("\")

# Entries of the BepInEx archive that must not be shipped. Empty, and kept
# because the question is worth answering once in writing rather than twice by
# guess.
#
# changelog.txt was excluded here until 2026-08-26 on the belief that the game
# ships one at the root of its folder and BepInEx's would overwrite it. It does
# not. A pristine depot download of build 23980936 has no changelog.txt at all,
# and the changelog.txt in both modded copies on this machine is byte-identical
# to the one in BepInEx's archive - same SHA256, same 7558 bytes - so that file
# was BepInEx's the whole time. Same story for winhttp.dll, doorstop_config.ini
# and the dotnet\ folder: absent from the pristine copy, all three BepInEx's.
#
# So the bundle ships BepInEx's archive whole. If something ever does have to be
# left out, add it here and say what it would have collided with.
$BepInExZipExcludes = @()

# Entries RENAMED on the way into the bundle.
#
# changelog.txt is BepInEx's own build changelog, and at the root of a game
# folder that bare name says nothing about whose it is - which is the same
# confusion that nearly had it excluded outright. Prefixing it is what the
# ViewSelected mod does with the same file, and it lets a player see at a glance
# which files arrived with this bundle.
$BepInExZipRenames = @{
    "changelog.txt" = "BepInEx-changelog.txt"
}

# What the bundle is called, and the files it adds beside the game exe so an
# install can be undone and its terms read.
$BundleSuffix = "AllInOne"
$UninstallerName = "Uninstall-$AssemblyName.ps1"
$InstallManifestName = "$AssemblyName-install-manifest.json"
$ThirdPartyNoticeName = "$AssemblyName-THIRD-PARTY-NOTICES.txt"
$UninstallerSource = Join-Path $RepoRoot "packaging\$UninstallerName"

# This project's own licence, and the name it takes inside a release archive.
#
# Shipped in BOTH archives, not just kept in the repository. MIT asks that the
# notice travel with the software, and a DLL sitting in somebody's game folder
# is a distribution: whoever holds it should be able to read the terms without
# going to find the source. The plugin-only zip carries it for exactly the same
# reason the all-in-one does.
$LicenseSource = Join-Path $RepoRoot "LICENSE"
$LicenseReleaseName = "$AssemblyName-LICENSE.txt"

# BepInEx's licence, shipped as a file of its own at the archive root rather
# than pasted into the notice, so it reads as a licence rather than as prose.
$BepInExLicenseReleaseName = "BepInEx-LICENSE.txt"

# Steam AppID for Disco Elysium / The Final Cut (from appmanifest_632470.acf).
$DiscoElysiumAppId = 632470
# Conventional steamapps\common folder name, used if the manifest is unreadable.
$DiscoElysiumInstallDirName = "Disco Elysium"
# Present in every Disco Elysium install; used to sanity-check a candidate path.
$GameExeName = "disco.exe"

# Sub-paths of a game install, relative to its root.
$BepInExCoreRelDir = "BepInEx\core"
$BepInExInteropRelDir = "BepInEx\interop"
$BepInExPluginsRelDir = "BepInEx\plugins"
$BepInExLogRelPath = "BepInEx\LogOutput.log"
$BepInExConfigRelPath = "BepInEx\config\BepInEx.cfg"
# One folder per plugin under BepInEx\plugins; this is ours.
$PluginFolderName = $AssemblyName
# What an installed payload is made of: $AssemblyName*.dll, and nothing else.
# The .pdb is not shipped - it is a debugging artefact of the machine that built
# it, and a player has nothing to do with it. The cost is that a stack trace in
# a player's log carries no line numbers; the commit stamped inside the DLL says
# which source those traces belong to, and a local build keeps its .pdb beside
# it for anyone actually debugging.
#
# ONE set, used at both ends: the copy in and the clear out of a previous
# install (see Get-PluginPayloadFile). That symmetry is the property worth
# keeping - the files a deploy removes are exactly the files it writes, so
# neither end can surprise the other. It does mean a .pdb an older build
# installed is left where it is rather than cleaned up, on the machines that
# have one. That is accepted: it is a stray file on a handful of development
# installs, not something a player will ever see.
$PluginPayloadExtensions = @(".dll")
# Appended to the commit hash the build stamps into the plugin assembly when the
# tree it was built from differed from that commit in a way the build could see.
# Written by Get-SourceRevisionId and read back by Get-PluginBuildStamp, which is
# why it lives here and not in either of them.
$PluginCommitDirtySuffix = ".dirty"

# The two Win32 error codes that mean "another process has this file open", as
# they arrive in the low word of an IOException's HResult. See
# Invoke-WithFileRetry, which retries these and nothing else.
$SharingViolation = 32
$LockViolation = 33

# Read-only reference material kept in this repo: game copies straight from
# Steam, AssetRipper exports, decompiler output. Builds must never write into
# any of it, and deploy refuses to target it unless explicitly forced.
#
# The FOLDER is the rule, not a list of names. Everything reference-ish moved
# under .game_reference_copies on 2026-08-26, so a copy added tomorrow is
# protected without anyone remembering to add it here. The old individual name
# stays beside it, for a checkout that still has one at the repo root.
$GameRefCopiesDirName = ".game_reference_copies"
$ReferenceCopyDirNames = @(
    $GameRefCopiesDirName,
    "Steam Install - Unaltered"
)
$GameRefCopiesDir = Join-Path $RepoRoot $GameRefCopiesDirName

# Mirrors step 3 of the MSBuild resolution in Directory.Build.props. A copy in
# there only counts if it has BepInEx in it - Test-ReferenceGameDir decides -
# because most of what lives in that folder now is PRISTINE game content, which
# carries none of the assemblies this build references.
$RepoDefaultGameDir = Join-Path $GameRefCopiesDir "Steam Install - Unaltered\Disco Elysium"

# Maps the csproj's HintPath properties onto directories under a game install,
# so Get-RequiredReferenceDll can read the reference list out of the csproj
# instead of duplicating it here.
$HintPathVarToRelDir = @{
    "BepInExCoreDir"    = $BepInExCoreRelDir
    "BepInExInteropDir" = $BepInExInteropRelDir
}


# --- Script plumbing ----------------------------------------------------------

function Invoke-ScriptMain {
    # Run an entry script's main flow, turning any thrown error into just its
    # message plus exit code 1. These scripts throw messages written for a human
    # to act on, and PowerShell's default dump buries them in position info and
    # a repeat of the whole message.
    param([Parameter(Mandatory = $true)][scriptblock]$Body)
    try {
        & $Body
    }
    catch {
        Write-Host ""
        Write-Host $_.Exception.Message -ForegroundColor Red
        exit 1
    }
}


function Get-PluginInstallDir {
    # Where this plugin's files live inside a game install.
    param([Parameter(Mandatory = $true)][string]$GameDir)
    return Join-Path $GameDir "$BepInExPluginsRelDir\$PluginFolderName"
}


function Get-RequiredReferenceDll {
    # Every reference DLL the csproj expects, as full paths under $GameDir.
    # Parsed out of the csproj so the csproj stays the single source of truth.
    param([Parameter(Mandatory = $true)][string]$GameDir)

    $text = [System.IO.File]::ReadAllText($ProjectFile)
    $pattern = 'HintPath="\$\((?<var>\w+)\)(?<file>[^"]+)"'
    $paths = [System.Collections.Generic.List[string]]::new()
    foreach ($m in [regex]::Matches($text, $pattern)) {
        $var = $m.Groups["var"].Value
        if (-not $HintPathVarToRelDir.ContainsKey($var)) {
            throw "Unrecognized HintPath property '$var' in $ProjectFile; update `$HintPathVarToRelDir in build-support.psm1."
        }
        $paths.Add((Join-Path $GameDir (Join-Path $HintPathVarToRelDir[$var] $m.Groups["file"].Value)))
    }
    if ($paths.Count -eq 0) {
        throw "No <Reference HintPath=...> entries found in $ProjectFile."
    }
    return $paths
}


function Test-ReferenceGameDir {
    # Cheap check that a candidate path is a game install with BepInEx set up
    # far enough to build against (core + generated interop assemblies).
    param([string]$Path)
    if (-not $Path) { return $false }
    foreach ($rel in @($BepInExCoreRelDir, $BepInExInteropRelDir)) {
        if (-not (Test-Path -LiteralPath (Join-Path $Path $rel))) { return $false }
    }
    return $true
}


function Get-SteamPath {
    # Steam records its own location in the registry; return it, or $null if
    # Steam is not installed. HKCU first (per-user, the active install), then
    # the 32-bit HKLM fallback.
    foreach ($probe in @(
            @{ Path = "HKCU:\Software\Valve\Steam"; Name = "SteamPath" },
            @{ Path = "HKLM:\SOFTWARE\WOW6432Node\Valve\Steam"; Name = "InstallPath" }
        )) {
        try {
            $value = (Get-ItemProperty -Path $probe.Path -Name $probe.Name -ErrorAction Stop).($probe.Name)
            if ($value) { return $value }
        }
        catch {
            # Key/value absent - try the next probe.
        }
    }
    return $null
}


function Get-SteamLibraryFolder {
    # Every Steam library root: Steam's own, plus the "path" entries in
    # libraryfolders.vdf (modern config\ location and legacy steamapps\ one).
    # Games can live on other drives, so the default library is not enough.
    param([Parameter(Mandatory = $true)][string]$SteamPath)

    $roots = [System.Collections.Generic.List[string]]::new()
    $roots.Add($SteamPath)

    foreach ($rel in @("config\libraryfolders.vdf", "steamapps\libraryfolders.vdf")) {
        $vdf = Join-Path $SteamPath $rel
        if (-not (Test-Path -LiteralPath $vdf)) { continue }
        $text = [System.IO.File]::ReadAllText($vdf)
        foreach ($m in [regex]::Matches($text, '"path"\s*"([^"]+)"')) {
            # VDF escapes backslashes as "\\"; unescape to a real Windows path.
            $roots.Add(($m.Groups[1].Value -replace '\\\\', '\'))
        }
    }

    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    return $roots | Where-Object { $_ -and $seen.Add($_) }
}


function Find-SteamGameDir {
    # Auto-discover the Steam copy of Disco Elysium (the folder holding
    # disco.exe). Returns $null if Steam or the game is not found - callers
    # decide whether that is fatal, since nothing here is ever a silent default
    # for a destructive operation.
    $steamPath = Get-SteamPath
    if (-not $steamPath) { return $null }

    foreach ($lib in (Get-SteamLibraryFolder -SteamPath $steamPath)) {
        # Prefer the installdir recorded in the app manifest; fall back to the
        # conventional folder name.
        $installDirs = [System.Collections.Generic.List[string]]::new()
        $acf = Join-Path $lib "steamapps\appmanifest_$DiscoElysiumAppId.acf"
        if (Test-Path -LiteralPath $acf) {
            $m = [regex]::Match([System.IO.File]::ReadAllText($acf), '"installdir"\s*"([^"]+)"')
            if ($m.Success) { $installDirs.Add($m.Groups[1].Value) }
        }
        $installDirs.Add($DiscoElysiumInstallDirName)

        foreach ($installDir in $installDirs) {
            $gameDir = Join-Path $lib "steamapps\common\$installDir"
            if (Test-Path -LiteralPath (Join-Path $gameDir $GameExeName)) {
                return $gameDir
            }
        }
    }
    return $null
}


function Test-IsReferenceCopy {
    # True if $Path is inside one of the repo's read-only reference copies of
    # the game. Compared per path segment so a merely similar name cannot match.
    param([Parameter(Mandatory = $true)][string]$Path)
    $full = [System.IO.Path]::GetFullPath($Path)
    $segments = $full -split '[\\/]+'
    foreach ($name in $ReferenceCopyDirNames) {
        if ($segments -contains $name) { return $true }
    }
    return $false
}


function Assert-NotReferenceCopy {
    # Hard stop before writing anywhere inside a reference copy of the game.
    param([Parameter(Mandatory = $true)][string]$Path)
    if (Test-IsReferenceCopy -Path $Path) {
        throw @"
Refusing to write into a reference copy of the game:
  $Path
Reference copies (currently: $($ReferenceCopyDirNames -join ', ')) are read-only
material for this repo. Deploy into a playable copy of the game instead, e.g.
the Steam install.
"@
    }
}


function Resolve-TargetGameDir {
    # The playable game folder a command should act on - the one deploy writes
    # into and capture-log reads the session log out of. An override if given,
    # else the auto-discovered Steam copy. Returns a validated path, or throws
    # with instructions when there is genuinely no install to act on.
    #
    # Unrelated to Resolve-ReferenceGameDir below, which picks the install the
    # build reads its reference assemblies from.
    param([string]$GameDir)

    $target = $null
    $source = $null
    if ($GameDir) {
        $target = $GameDir
        $source = "-GameDir"
    }
    elseif ($env:DISCO_ELYSIUM_DEPLOY_DIR) {
        $target = $env:DISCO_ELYSIUM_DEPLOY_DIR
        $source = "`$env:DISCO_ELYSIUM_DEPLOY_DIR"
    }
    else {
        $target = Find-SteamGameDir
        $source = "Steam auto-discovery (AppID $DiscoElysiumAppId)"
        if (-not $target) {
            throw @"
No game folder to act on: no Steam copy of Disco Elysium (AppID $DiscoElysiumAppId) was found, and no override was given.
Name one explicitly:
  -GameDir "<path to game folder>"
  `$env:DISCO_ELYSIUM_DEPLOY_DIR = "<path to game folder>"
"@
        }
    }

    if (-not (Test-Path -LiteralPath (Join-Path $target $GameExeName))) {
        throw "Game folder from $source does not look like a Disco Elysium install (no $GameExeName): $target"
    }
    Write-Host "Game folder from ${source}: $target"
    return (Get-Item -LiteralPath $target).FullName
}


function Save-ReferenceGameDir {
    # Record a resolved reference install as this machine's cached answer.
    #
    # Called for EVERY route that resolves one, not just Steam discovery: the
    # cache is what a checkout with no reference copy of its own reads - an
    # agent worktree above all, and Directory.Build.props reads it directly - so
    # the useful moment to write it is whenever the answer is known, including
    # the ordinary build in the main checkout that never gets as far as
    # discovery. Best effort: a machine that will not let us write here still
    # builds, it just re-resolves every time.
    param([Parameter(Mandatory = $true)][string]$GameDir)
    if (-not $env:LOCALAPPDATA) { return }
    try {
        $full = (Get-Item -LiteralPath $GameDir).FullName
        if ((Test-Path -LiteralPath $RefDirCacheFile) -and
            (Get-Content -LiteralPath $RefDirCacheFile -Raw).Trim() -eq $full) {
            return
        }
        New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
        Set-Content -LiteralPath $RefDirCacheFile -Value $full -Encoding ascii
    }
    catch {
        Write-Verbose "Could not cache the reference game dir at ${RefDirCacheFile}: $($_.Exception.Message)"
    }
}


function Resolve-ReferenceGameDir {
    # The game install the build reads its reference assemblies from.
    #
    # Order: explicit parameter, DISCO_ELYSIUM_DIR, the LIVE STEAM INSTALL, the
    # cached previous answer, then a copy kept in .game_reference_copies. The
    # live install comes before either stored answer on purpose - it is the one
    # that is patched, re-run and regenerated as the game updates, so it is the
    # one whose interop assemblies match the game a developer is actually
    # playing. Read-only use, so preferring it is safe.
    #
    # What CANNOT satisfy this is a pristine copy from depot_download.ps1. Those
    # are the game as Steam ships it, with no BepInEx in them at all, and what
    # the build needs is BepInEx\core plus the BepInEx\interop assemblies that
    # only exist once BepInEx has been installed into a copy and the game run
    # once. Test-ReferenceGameDir is what enforces that, and it is why a folder
    # full of reference copies can still leave this throwing.
    #
    # Whatever it resolves to is written to the machine-level cache on the way
    # out, which is what lets a git worktree build at all: Directory.Build.props
    # reads that same file, and MSBuild cannot discover Steam for itself.
    param([string]$DiscoElysiumDir)

    if ($DiscoElysiumDir) {
        if (-not (Test-ReferenceGameDir -Path $DiscoElysiumDir)) {
            throw "-DiscoElysiumDir '$DiscoElysiumDir' has no $BepInExCoreRelDir + $BepInExInteropRelDir. Point it at a Disco Elysium install that has been run once with BepInEx 6."
        }
        Save-ReferenceGameDir -GameDir $DiscoElysiumDir
        return $DiscoElysiumDir
    }
    if ($env:DISCO_ELYSIUM_DIR) {
        if (-not (Test-ReferenceGameDir -Path $env:DISCO_ELYSIUM_DIR)) {
            throw "DISCO_ELYSIUM_DIR='$($env:DISCO_ELYSIUM_DIR)' has no $BepInExCoreRelDir + $BepInExInteropRelDir. Point it at a Disco Elysium install that has been run once with BepInEx 6."
        }
        Save-ReferenceGameDir -GameDir $env:DISCO_ELYSIUM_DIR
        return $env:DISCO_ELYSIUM_DIR
    }
    $steamDir = Find-SteamGameDir
    if ($steamDir -and (Test-ReferenceGameDir -Path $steamDir)) {
        Save-ReferenceGameDir -GameDir $steamDir
        return $steamDir
    }

    $cachedGameDir = $null
    if (Test-Path -LiteralPath $RefDirCacheFile) {
        $cachedGameDir = (Get-Content -LiteralPath $RefDirCacheFile -Raw).Trim()
        if (Test-ReferenceGameDir -Path $cachedGameDir) { return $cachedGameDir }
    }

    if (Test-ReferenceGameDir -Path $RepoDefaultGameDir) {
        Save-ReferenceGameDir -GameDir $RepoDefaultGameDir
        return $RepoDefaultGameDir
    }

    $cacheNote = if (-not $cachedGameDir) {
        "(no cache file at $RefDirCacheFile)"
    }
    else {
        "$cachedGameDir (cached, rejected: no $BepInExCoreRelDir + $BepInExInteropRelDir)"
    }
    $steamNote = if ($steamDir) {
        "The Steam install at '$steamDir' has no $BepInExInteropRelDir - run the game once with BepInEx 6 installed to generate the interop assemblies."
    }
    else {
        "No Steam copy of Disco Elysium (AppID $DiscoElysiumAppId) was found."
    }
    throw @"
Could not find a Disco Elysium install to build against.
Tried, in order:
  1. -DiscoElysiumDir                      (not given)
  2. `$env:DISCO_ELYSIUM_DIR               (not set)
  3. Steam auto-discovery
  4. $cacheNote
  5. $RepoDefaultGameDir
$steamNote
What this needs is an install with BOTH $BepInExCoreRelDir and
$BepInExInteropRelDir - a copy with BepInEx 6 installed that has been RUN once,
so BepInEx has generated the interop assemblies from it. A pristine copy from
depot_download.ps1 is not that: it is the game as Steam ships it, with no
BepInEx in it. Downloading one is the first half of making a reference install,
not the whole of it.

So: install BepInEx 6 (IL2CPP) into a copy and launch the game once, then pass
-DiscoElysiumDir <path> or set DISCO_ELYSIUM_DIR.
"@
}


function Initialize-BuildReferences {
    # Resolve the reference install and prove every DLL the csproj references is
    # actually there, so a missing reference fails here with a clear message
    # rather than as a wall of MSBuild errors. Returns the resolved game dir.
    param([string]$DiscoElysiumDir)

    $gameDir = Resolve-ReferenceGameDir -DiscoElysiumDir $DiscoElysiumDir
    Write-Host "Reference install: $gameDir"

    $missing = Get-RequiredReferenceDll -GameDir $gameDir |
        Where-Object { -not (Test-Path -LiteralPath $_) }
    if ($missing) {
        throw @"
Reference install is missing $($missing.Count) assembly/assemblies the build needs:
$($missing -join "`n")
If the interop ones are missing, run the game once with BepInEx 6 installed so
it regenerates $BepInExInteropRelDir.
"@
    }
    Write-Host "References: all present."
    return $gameDir
}


# --- Building and packaging ---------------------------------------------------

function Test-BuildAffectingUntracked {
    # Whether an untracked path could end up in the build.
    #
    # Everything the compiler reads lives under src\ or tools\, plus the build
    # inputs at the repo root - the solution and the Directory.Build.* files. An
    # untracked file anywhere else (a stray debug.log, a scratch note, an
    # exported save) cannot change a single byte of output.
    param([Parameter(Mandatory = $true)][string]$Path)
    $normalized = $Path -replace "\\", "/"
    return $normalized -like "src/*" -or
    $normalized -like "tools/*" -or
    $normalized -like "Directory.Build.*" -or
    $normalized -like "*.slnx"
}


function Get-SourceRevisionStatus {
    # What the build can say about its own origin: the commit, whether the tree
    # differs from it in a way that affects the build, and what those
    # differences are. $null when git cannot answer, which is not an error: a
    # source drop without a .git still builds, it just produces an assembly that
    # cannot name its origin.
    #
    # Tracked changes always count. Untracked files count only if they could be
    # compiled (see Test-BuildAffectingUntracked): an untracked .cs under src\
    # is compiled like any other, so a tree holding one is not the commit it
    # would otherwise claim - but a stray file the build never reads is not a
    # reason to call every build of a clean checkout ".dirty", which is what
    # used to happen and what made the flag worth nothing.
    try {
        $commit = (& git -C $RepoRoot rev-parse HEAD 2>$null)
        if ($LASTEXITCODE -ne 0 -or -not $commit) { return $null }
        $status = @(& git -C $RepoRoot status --porcelain 2>$null)
        if ($LASTEXITCODE -ne 0) { return $null }
    }
    catch {
        # No git on PATH.
        return $null
    }

    $tracked = [System.Collections.Generic.List[string]]::new()
    $untracked = [System.Collections.Generic.List[string]]::new()
    $ignoredUntracked = [System.Collections.Generic.List[string]]::new()
    foreach ($line in $status) {
        if (-not $line) { continue }
        # Porcelain v1: two status columns, a space, then the path. A rename
        # reads "old -> new"; the new name is the one that is on disk.
        $path = $line.Substring(3).Trim('"')
        if ($path -match "^.* -> (?<new>.*)$") { $path = $Matches.new.Trim('"') }
        if ($line.StartsWith("??")) {
            if (Test-BuildAffectingUntracked -Path $path) { $untracked.Add($path) }
            else { $ignoredUntracked.Add($path) }
        }
        else {
            $tracked.Add($path)
        }
    }

    $reasons = [System.Collections.Generic.List[string]]::new()
    if ($tracked.Count -gt 0) {
        $reasons.Add("$($tracked.Count) tracked file(s) changed: $($tracked -join ', ')")
    }
    if ($untracked.Count -gt 0) {
        $reasons.Add("$($untracked.Count) untracked file(s) the build reads: $($untracked -join ', ')")
    }

    return [pscustomobject]@{
        Commit           = "$commit".Trim()
        Dirty            = $reasons.Count -gt 0
        Reasons          = $reasons.ToArray()
        IgnoredUntracked = $ignoredUntracked.ToArray()
    }
}


function Get-SourceRevisionId {
    # The commit this repo is at, with $PluginCommitDirtySuffix appended when the
    # tree differs from it in a way that affects the build, in the exact form the
    # build stamps into the plugin assembly (see Invoke-PluginBuild). $null when
    # git cannot answer. See Get-SourceRevisionStatus for what counts.
    param($Status)
    if (-not $PSBoundParameters.ContainsKey("Status")) { $Status = Get-SourceRevisionStatus }
    if (-not $Status) { return $null }
    $revision = $Status.Commit
    if ($Status.Dirty) { $revision += $PluginCommitDirtySuffix }
    return $revision
}


function Get-CachedDownload {
    # A file fetched once per machine and kept, verified by SHA256 when a hash
    # is given. Cached beside the reference-game-dir answer, and for the same
    # reason: a 34 MB download per release build, per checkout, is a tax nobody
    # should pay twice.
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$FileName,
        [string]$Sha256,
        [string]$What = "file"
    )

    New-Item -ItemType Directory -Force -Path $BepInExCacheDir | Out-Null
    $path = Join-Path $BepInExCacheDir $FileName

    if (Test-Path -LiteralPath $path) {
        if (-not $Sha256) { return $path }
        $have = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($have -eq $Sha256.ToLowerInvariant()) { return $path }
        Write-Warning "Cached $What at $path does not match its expected hash; downloading it again."
        Remove-Item -LiteralPath $path -Force
    }

    Write-Host "Downloading $What"
    Write-Host "  from $Url"
    $temp = "$path.partial"
    try {
        # Written to a .partial and moved into place, so an interrupted download
        # cannot leave a truncated file that the next run trusts.
        Invoke-WebRequest -Uri $Url -OutFile $temp -UseBasicParsing
        if ($Sha256) {
            $have = (Get-FileHash -LiteralPath $temp -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($have -ne $Sha256.ToLowerInvariant()) {
                throw @"
Downloaded $What does not match its pinned hash.
  url      $Url
  expected $Sha256
  got      $have
Refusing to package it. Either the artifact was replaced upstream or something
is rewriting the download; check before changing the pinned hash.
"@
            }
        }
        Move-Item -LiteralPath $temp -Destination $path -Force
    }
    finally {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
    }
    Write-Host "  cached at $path"
    return $path
}


function Get-BepInExBundleZip {
    # The pinned BepInEx archive the all-in-one bundle is built from.
    return Get-CachedDownload -Url $BepInExZipUrl -FileName $BepInExZipName `
        -Sha256 $BepInExZipSha256 -What "BepInEx $BepInExVersion"
}


function Get-BepInExLicense {
    # BepInEx's LICENSE at the pinned commit. No hash: the URL names an immutable
    # commit, so the content cannot change under it.
    return Get-CachedDownload -Url $BepInExLicenseUrl -FileName "BepInEx-LICENSE-$BepInExShortCommit.txt" `
        -What "the BepInEx licence"
}


function Get-FileLockHolder {
    # Who has this file open, as "name (pid N)", or $null if that cannot be
    # answered. Best effort and quiet: this only ever decorates an error
    # message, so nothing here may throw or slow a failure down.
    #
    # Sysinternals handle.exe is the only thing on Windows that answers the
    # question without writing a kernel driver, and it is not something this
    # repo can require. When it is absent - or refuses without elevation - the
    # caller says "something else has it open" and lists the usual suspects,
    # which is what it would have said anyway.
    param([Parameter(Mandatory = $true)][string]$Path)

    $handle = Get-Command handle64.exe, handle.exe -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty Source
    if (-not $handle) { return $null }

    try {
        $name = [System.IO.Path]::GetFileName($Path)
        $output = & $handle -nobanner -a $name 2>$null
        # Lines read: "7zFM.exe   pid: 32520  type: File   2EC: D:\...\thing.zip"
        foreach ($line in @($output)) {
            if ($line -match "^(?<proc>\S+)\s+pid:\s*(?<pid>\d+).*\s(?<path>\S:\\.*)$" -and
                $Matches.path -like "*$name") {
                return "$($Matches.proc) (pid $($Matches.pid))"
            }
        }
    }
    catch {
        # Not installed, not permitted, output in a shape we do not know: all of
        # them mean the same thing here, which is that we cannot name the holder.
    }
    return $null
}


function Invoke-WithFileRetry {
    # Run a file operation that can fail only because something else has the
    # file open, retrying a few times before giving up with a message a human
    # can act on.
    #
    # The case this exists for: a just-written .zip is exactly what an on-access
    # virus scanner or an Explorer preview handler opens, and it holds the file
    # for a second or two. Left alone that surfaces as .NET's "The process
    # cannot access the file ... because it is being used by another process",
    # which says nothing about what to do and looks like a bug in the packaging.
    #
    # Only locking failures are retried. Anything else - a bad path, a full
    # disk, a permissions problem - is thrown straight away, since retrying it
    # would only delay the same error. "IOException" is not a good enough test
    # for that: DirectoryNotFoundException and FileNotFoundException both derive
    # from it, so the check is on the Win32 code in the low word of HResult -
    # 32 ERROR_SHARING_VIOLATION, 33 ERROR_LOCK_VIOLATION - which is what a file
    # held open by another process actually raises.
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Operation,
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$What = "write",
        [int]$Attempts = 4,
        [double]$DelaySeconds = 1.5
    )

    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try {
            & $Operation
            return
        }
        catch [System.IO.IOException] {
            $win32 = $_.Exception.HResult -band 0xFFFF
            if ($win32 -ne $SharingViolation -and $win32 -ne $LockViolation) {
                # Not a lock, so nothing here will change on a second attempt.
                throw
            }
            if ($attempt -eq $Attempts) {
                $holder = Get-FileLockHolder -Path $Path
                $who = if ($holder) {
                    "$holder has it open."
                }
                else {
                    "Something else has it open - an archive viewer browsing the file (7-Zip's File Manager holds one open the whole time it is listed), an Explorer preview, or a virus scanner."
                }
                throw @"
Could not $What '$Path'.
$who
Tried $Attempts times over $([math]::Round(($Attempts - 1) * $DelaySeconds, 1)) s. A scanner or a preview lets go
on its own; a window someone left open does not. Close it and run this again.
Underlying error: $($_.Exception.Message)
"@
            }
            Write-Host "  $Path is in use; retrying in $DelaySeconds s ($attempt of $($Attempts - 1))..."
            Start-Sleep -Seconds $DelaySeconds
        }
    }
}


function Expand-BepInExInto {
    # Extract the pinned BepInEx archive into a staging folder, minus the
    # entries that would collide with the game's own files.
    param(
        [Parameter(Mandatory = $true)][string]$StageDir,
        [Parameter(Mandatory = $true)][string]$ZipPath
    )

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        $skipped = 0
        foreach ($entry in $zip.Entries) {
            $relative = $entry.FullName -replace "/", "\"
            if ($BepInExZipExcludes -contains $relative) {
                $skipped++
                continue
            }
            if ($BepInExZipRenames.ContainsKey($relative)) {
                $relative = $BepInExZipRenames[$relative]
            }
            $target = Join-Path $StageDir $relative
            if (-not $entry.Name) {
                # A directory entry: BepInEx ships empty plugins\ and patchers\,
                # and an install wants them there rather than created on demand.
                New-Item -ItemType Directory -Force -Path $target | Out-Null
                continue
            }
            New-Item -ItemType Directory -Force -Path ([System.IO.Path]::GetDirectoryName($target)) | Out-Null
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
        $skippedNote = if ($skipped -gt 0) { ", $skipped skipped: $($BepInExZipExcludes -join ', ')" } else { "" }
        $renamedNote = if ($BepInExZipRenames.Count -gt 0) {
            ", renamed: $(($BepInExZipRenames.GetEnumerator() | ForEach-Object { "$($_.Key) -> $($_.Value)" }) -join ', ')"
        }
        else { "" }
        Write-Host "  BepInEx $BepInExVersion ($($zip.Entries.Count - $skipped) files$skippedNote$renamedNote)"
    }
    finally {
        $zip.Dispose()
    }
}


function New-InstallManifest {
    # A record of exactly what a staged bundle contains, hashed, so the shipped
    # uninstaller can remove those files and only those files - and only while
    # they still hold the bytes that were shipped.
    #
    # The directory list is what the uninstaller may prune once it has emptied
    # them, deepest first. Directories are recorded rather than derived so that
    # an install into a folder that already had, say, a BepInEx\plugins can
    # never see it removed: the list holds only what this bundle created.
    param(
        [Parameter(Mandatory = $true)][string]$StageDir,
        [Parameter(Mandatory = $true)][string]$Version,
        [Parameter(Mandatory = $true)][string]$ZipSha256
    )

    $prefix = (Get-Item -LiteralPath $StageDir).FullName.TrimEnd("\") + "\"
    $files = foreach ($file in (Get-ChildItem -LiteralPath $StageDir -Recurse -File -Force)) {
        $relative = $file.FullName.Substring($prefix.Length)
        # The manifest names itself and cannot hash itself; the uninstaller
        # deletes it explicitly at the end instead.
        if ($relative -eq $InstallManifestName) { continue }
        [ordered]@{
            path   = $relative -replace "\\", "/"
            sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            bytes  = $file.Length
        }
    }

    $directories = foreach ($dir in (Get-ChildItem -LiteralPath $StageDir -Recurse -Directory -Force)) {
        ($dir.FullName.Substring($prefix.Length)) -replace "\\", "/"
    }

    return [ordered]@{
        bundle        = $AssemblyName
        bundleVersion = $Version
        createdAt     = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
        pluginCommit  = Get-SourceRevisionId
        bepInEx       = [ordered]@{
            version = $BepInExVersion
            commit  = $BepInExCommit
            source  = $BepInExZipUrl
            sha256  = $ZipSha256
        }
        files         = @($files)
        directories   = @($directories)
    }
}


function Copy-PluginLicense {
    # This project's MIT licence, into a staged archive. Shared by both archives
    # so neither can quietly ship a binary with no terms attached, and a missing
    # LICENSE is an error rather than a silently licence-less release.
    param([Parameter(Mandatory = $true)][string]$StageDir)

    if (-not (Test-Path -LiteralPath $LicenseSource)) {
        throw "Missing $LicenseSource, so a release would ship without its licence. Restore it before packaging."
    }
    Copy-Item -LiteralPath $LicenseSource -Destination (Join-Path $StageDir $LicenseReleaseName)
    Write-Host "  $LicenseReleaseName"
}


function New-ThirdPartyNotice {
    # What is in the bundle that this repo did not write, and under what terms.
    # LGPL-2.1 asks for the licence text, for the recipient to know what they
    # have, and for the source to be available; BepInEx is redistributed here
    # unmodified, so naming the exact build, its commit and where it came from
    # covers all three.
    #
    # The licence text itself is a separate file at the archive root - see
    # $BepInExLicenseReleaseName - and this notice points at it. Attribution and
    # licence are two different jobs, and a reader looking for the terms should
    # find a licence file rather than a licence quoted inside an essay.
    param(
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $notice = @"
THIRD-PARTY NOTICES
===================

BepInEx $BepInExVersion (IL2CPP, win-x64)

  Redistributed unmodified, exactly as published at:
    $BepInExZipUrl

  SHA256 of that archive:
    $BepInExZipSha256

  Built from commit $BepInExCommit
  Source: https://github.com/BepInEx/BepInEx/tree/$BepInExCommit

  License: GNU Lesser General Public License v2.1
           see $BepInExLicenseReleaseName in this archive for the full text

  BepInEx's archive is shipped whole - every file it publishes, nothing added
  and nothing left out - with one file renamed for clarity at the game root:
  changelog.txt, BepInEx's own build changelog, ships as BepInEx-changelog.txt.

  Several things this bundle puts at the root of the game folder are BepInEx's
  rather than the game's, which is easy to get backwards: winhttp.dll,
  doorstop_config.ini, .doorstop_version, BepInEx-changelog.txt and the
  ``dotnet`` folder, which is the CoreCLR runtime its IL2CPP loader needs. A
  pristine copy of the game straight from Steam has none of them. The
  uninstaller removes them along with everything else this bundle wrote.

$AssemblyName itself
$("=" * ($AssemblyName.Length + 6))

  A separate work that uses BepInEx as a plugin host; bundling the two here is
  for the convenience of anyone who does not already have BepInEx installed.

  Copyright (c) 2026 Paul Molodowitch
  License: MIT - see $LicenseReleaseName in this archive
  Source:  https://github.com/pmolodo/DiscoElysium_GlobalConversationTracker

  The plugin is the one file under BepInEx\plugins\$AssemblyName\; every other
  file in this archive belongs to BepInEx.
"@
    [System.IO.File]::WriteAllText($Destination, $notice)
}


function New-AllInOneBundle {
    # Build the archive a player with a stock, unmodded install can extract over
    # their game folder: BepInEx, this plugin, the licence that comes with
    # redistributing BepInEx, and an uninstaller that can undo all of it.
    #
    # Kept here rather than in make-release.ps1 so it can be built to any
    # destination - which is what lets it be tested against a scratch game
    # folder instead of only as part of a release.
    #
    # Returns the path to the archive.
    param(
        [Parameter(Mandatory = $true)][string]$DllPath,
        [Parameter(Mandatory = $true)][string]$Version,
        [Parameter(Mandatory = $true)][string]$ZipPath,
        [Parameter(Mandatory = $true)][string]$StageDir,
        [Parameter(Mandatory = $true)][string]$ReadmeSource,
        [Parameter(Mandatory = $true)][string]$ReadmeName
    )

    if (-not (Test-Path -LiteralPath $UninstallerSource)) {
        throw "Missing the uninstaller this bundle ships: $UninstallerSource"
    }
    if (-not (Test-Path -LiteralPath $ReadmeSource)) {
        throw "Missing plugin README: $ReadmeSource"
    }

    $bepInExZip = Get-BepInExBundleZip
    $license = Get-BepInExLicense

    if (Test-Path -LiteralPath $StageDir) {
        Remove-Item -LiteralPath $StageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

    Write-Host "Staging all-in-one contents:"
    Expand-BepInExInto -StageDir $StageDir -ZipPath $bepInExZip
    Copy-PluginPayload -DllPath $DllPath -DestDir (Get-PluginInstallDir -GameDir $StageDir)
    Copy-Item -LiteralPath $ReadmeSource -Destination (Join-Path $StageDir $ReadmeName)
    Write-Host "  $ReadmeName"
    Copy-Item -LiteralPath $UninstallerSource -Destination (Join-Path $StageDir $UninstallerName)
    Write-Host "  $UninstallerName"

    # The licences, each as its own file at the archive root: ours because MIT
    # asks the notice to travel with the software, BepInEx's because LGPL-2.1
    # asks the same of a redistribution. The notice beside them says who wrote
    # what and points at both.
    Copy-PluginLicense -StageDir $StageDir
    Copy-Item -LiteralPath $license -Destination (Join-Path $StageDir $BepInExLicenseReleaseName)
    Write-Host "  $BepInExLicenseReleaseName"
    New-ThirdPartyNotice -Destination (Join-Path $StageDir $ThirdPartyNoticeName)
    Write-Host "  $ThirdPartyNoticeName"

    # Last, so it can hash everything else that is going in.
    $zipSha = (Get-FileHash -LiteralPath $bepInExZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifest = New-InstallManifest -StageDir $StageDir -Version $Version -ZipSha256 $zipSha
    $manifestPath = Join-Path $StageDir $InstallManifestName
    [System.IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 5))
    Write-Host "  $InstallManifestName ($($manifest.files.Count) files listed)"

    New-Item -ItemType Directory -Force -Path ([System.IO.Path]::GetDirectoryName($ZipPath)) | Out-Null
    try {
        if (Test-Path -LiteralPath $ZipPath) {
            Invoke-WithFileRetry -Path $ZipPath -What "replace" -Operation {
                Remove-Item -LiteralPath $ZipPath -Force -ErrorAction Stop
            }
        }
        $contents = Get-ChildItem -Force -LiteralPath $StageDir | ForEach-Object { $_.FullName }
        Invoke-WithFileRetry -Path $ZipPath -What "write" -Operation {
            Compress-Archive -Path $contents -DestinationPath $ZipPath -ErrorAction Stop
        }
    }
    finally {
        Remove-Item -LiteralPath $StageDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    return $ZipPath
}


function Get-CaptureManifestPath {
    # The manifest that belongs to a captured log. One spelling of the
    # convention, so the writer, the renamer and any reader agree on it.
    #
    # The pairing is BY NAME - <log>.capture.json sits beside <log> - and that
    # is the only pairing anything should rely on. A manifest's own 'copy' field
    # records where the copy was when it was written, which is history rather
    # than a pointer: rename the log and that path stops resolving, while the
    # name pairing still holds because Rename-Capture moves both together.
    param([Parameter(Mandatory = $true)][string]$LogPath)
    return "$LogPath.capture.json"
}


function Rename-Capture {
    # Rename a captured log and keep its manifest with it: the manifest moves to
    # match the new name, its 'copy' field is rewritten to where the copy now
    # is, and the old name is remembered in 'renamedFrom'.
    #
    # This exists because renaming a capture by hand is the obvious thing to do
    # - a log called capture-20260819-162547.log says nothing about the run it
    # documents - and doing it by hand leaves the manifest behind under the old
    # name, describing a path that no longer exists. Eight manifests in one
    # .build\logs folder had already been orphaned that way.
    #
    # Returns the new log path.
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$NewName
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "No captured log at $Path"
    }
    $log = Get-Item -LiteralPath $Path
    # A bare name renames in place; a path with a directory moves it there too.
    $newPath = if ([System.IO.Path]::GetDirectoryName($NewName)) {
        [System.IO.Path]::GetFullPath($NewName)
    }
    else {
        Join-Path $log.DirectoryName $NewName
    }
    if ($newPath -eq $log.FullName) { return $log.FullName }
    if (Test-Path -LiteralPath $newPath) {
        throw "Refusing to overwrite $newPath; rename to a name that is free."
    }

    $manifestPath = Get-CaptureManifestPath -LogPath $log.FullName
    $newManifestPath = Get-CaptureManifestPath -LogPath $newPath

    Move-Item -LiteralPath $log.FullName -Destination $newPath
    Write-Host "  $($log.Name) -> $([System.IO.Path]::GetFileName($newPath))"

    if (-not (Test-Path -LiteralPath $manifestPath)) {
        Write-Warning "No manifest at $manifestPath, so this capture carries no record of what it documents; the log itself was renamed."
        return $newPath
    }

    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $renamedFrom = @($manifest.renamedFrom) + @($manifest.copy) | Where-Object { $_ }
    $manifest | Add-Member -NotePropertyName renamedFrom -NotePropertyValue $renamedFrom -Force
    $manifest | Add-Member -NotePropertyName copy -NotePropertyValue $newPath -Force
    # WriteAllText rather than Set-Content: no BOM, matching how it was written.
    [System.IO.File]::WriteAllText($newManifestPath, ($manifest | ConvertTo-Json -Depth 5))
    Remove-Item -LiteralPath $manifestPath
    Write-Host "  $([System.IO.Path]::GetFileName($manifestPath)) -> $([System.IO.Path]::GetFileName($newManifestPath))"
    return $newPath
}


function Get-PluginBuildStamp {
    # What a built plugin assembly says about its own origin: the commit it was
    # compiled from, and whether that tree was dirty.
    #
    # The build passes the revision as SourceRevisionId, which the SDK appends to
    # the assembly's informational version and Windows exposes as the file's
    # ProductVersion - so the commit and the code it describes are the same file
    # and cannot drift apart. Fields come back $null for an assembly built
    # without a stamp, which is a fact to record rather than an error.
    param([Parameter(Mandatory = $true)][string]$DllPath)

    $stamp = [ordered]@{
        informationalVersion = $null
        version              = $null
        commit               = $null
        dirty                = $null
    }
    if (-not (Test-Path -LiteralPath $DllPath)) { return $stamp }

    $product = (Get-Item -LiteralPath $DllPath).VersionInfo.ProductVersion
    if (-not $product) { return $stamp }
    $stamp.informationalVersion = $product

    # "<version>+<revision>" - the SDK's own spelling of an informational version
    # carrying a SourceRevisionId. No "+" means nothing was stamped.
    $plus = $product.IndexOf("+")
    if ($plus -lt 0) {
        $stamp.version = $product
        return $stamp
    }
    $stamp.version = $product.Substring(0, $plus)

    $revision = $product.Substring($plus + 1)
    $stamp.dirty = $revision.EndsWith($PluginCommitDirtySuffix)
    if ($stamp.dirty) {
        $revision = $revision.Substring(0, $revision.Length - $PluginCommitDirtySuffix.Length)
    }
    $stamp.commit = $revision
    return $stamp
}


function Get-PluginVersion {
    # The plugin version, read from <Version> in the csproj (the source of truth
    # that BepInEx also reports at load time).
    $m = [regex]::Match([System.IO.File]::ReadAllText($ProjectFile), '<Version>([^<]+)</Version>')
    if (-not $m.Success) {
        throw "Could not find <Version> in $ProjectFile"
    }
    return $m.Groups[1].Value
}


function Invoke-PluginBuild {
    # Verify references, then `dotnet build`. Returns the path to the built
    # plugin DLL; throws if the build fails or the output is missing. `dotnet`
    # output goes to the host so it does not pollute the returned path.
    param(
        [string]$Configuration = "Release",
        [string]$DiscoElysiumDir
    )

    $gameDir = Initialize-BuildReferences -DiscoElysiumDir $DiscoElysiumDir

    # Stamped into the assembly so a deployed DLL - and any log captured from a
    # session that loaded it - can be tied back to the source it was built from.
    $buildArgs = @("-p:DiscoElysiumDir=$gameDir")
    $status = Get-SourceRevisionStatus
    $revision = Get-SourceRevisionId -Status $status
    if ($revision) {
        $buildArgs += "-p:SourceRevisionId=$revision"
        Write-Host "Source revision: $revision"
        # Say WHAT made it dirty, and what was deliberately not counted. A flag
        # with no explanation behind it is the thing this reporting exists to
        # stop: nobody can act on ".dirty" alone.
        foreach ($reason in $status.Reasons) {
            Write-Host "  dirty: $reason"
        }
        if ($status.IgnoredUntracked.Count -gt 0) {
            Write-Host "  ignored (untracked, outside the build): $($status.IgnoredUntracked -join ', ')"
        }
    }
    else {
        Write-Warning "Could not read the source revision from git; this build will carry no commit stamp, so a log captured from a session running it cannot name the source it came from."
    }

    Write-Host "Building $AssemblyName v$(Get-PluginVersion) ($Configuration)..."
    dotnet build $ProjectFile -c $Configuration @buildArgs | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet build failed with exit code $LASTEXITCODE"
    }

    $dllPath = Join-Path $BinDir "$Configuration\$TargetFramework\$AssemblyName.dll"
    if (-not (Test-Path -LiteralPath $dllPath)) {
        throw "Expected build output not found: $dllPath"
    }
    return $dllPath
}


function Get-PluginPayloadFile {
    # The plugin payload files present in $Directory - $AssemblyName*.dll and
    # nothing else - sorted by name, as FileInfo objects. Empty if the directory
    # does not exist.
    #
    # One predicate, used against both ends of an install: the build output
    # Copy-PluginPayload reads from, and the previous install Remove-PluginPayload
    # clears out. Keeping those the same set is what makes it safe for deploy.ps1
    # to delete files instead of the whole folder.
    param([Parameter(Mandatory = $true)][string]$Directory)
    if (-not (Test-Path -LiteralPath $Directory)) {
        return @()
    }
    return @(Get-ChildItem -LiteralPath $Directory -File |
        Where-Object { $_.Name -like "$AssemblyName*" -and $_.Extension -in $PluginPayloadExtensions } |
        Sort-Object Name)
}


function Remove-PluginPayload {
    # Remove a previous install's payload files from $DestDir, leaving everything
    # else in the folder untouched, and return how many files were removed.
    #
    # Deleting the whole plugin folder would also destroy
    # articy_ids_final_cut.json - the optional articy id map a user puts there
    # by hand, next to the plugin's own DLL. Only the files deploy.ps1 itself
    # wrote are its to delete.
    #
    # Selecting by the same predicate Copy-PluginPayload copies by, rather than by
    # the new build's file list, means an assembly a previous build produced and
    # this one no longer does is still cleared out - the separate
    # Core/Persistence/Session DLLs, for one. A .pdb from before the payload
    # narrowed to the DLL alone is the deliberate exception, and stays where it
    # is.
    param([Parameter(Mandatory = $true)][string]$DestDir)
    $files = @(Get-PluginPayloadFile -Directory $DestDir)
    foreach ($file in $files) {
        Remove-Item -LiteralPath $file.FullName -Force
    }
    return $files.Count
}


function Copy-PluginPayload {
    # Copy everything that makes up an installed plugin into $DestDir, creating it
    # if needed. Shared by deploy.ps1 and make-release.ps1 so an installed copy and
    # a packaged copy always hold the same files.
    #
    # That is the plugin DLL and nothing else. It is the only assembly there is -
    # the mod's own layers are compiled into it (see the plugin csproj) - and the
    # .pdb beside it in the build output is deliberately left there: debugging
    # symbols describe the machine that built them and are of no use in a player's
    # install. The separate Core/Persistence/Session DLLs an older build put in
    # that folder still match the predicate and are cleared out on the next
    # deploy; a .pdb from one no longer does, and is left alone.
    #
    # Only $AssemblyName* is copied: the game and BepInEx reference assemblies are
    # referenced with Private="false" and are not in the build output at all, so
    # there is nothing here that could drag a copy of the game's own DLLs along.
    param(
        [Parameter(Mandatory = $true)][string]$DllPath,
        [Parameter(Mandatory = $true)][string]$DestDir
    )
    New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
    $buildDir = [System.IO.Path]::GetDirectoryName($DllPath)
    $files = @(Get-PluginPayloadFile -Directory $buildDir)
    $dllName = [System.IO.Path]::GetFileName($DllPath)
    if (-not ($files | Where-Object { $_.Name -eq $dllName })) {
        throw "Plugin assembly $dllName was not among the files to copy from $buildDir"
    }
    foreach ($file in $files) {
        Copy-Item -LiteralPath $file.FullName -Destination $DestDir -Force
        Write-Host "  $($file.Name)"
    }
}


# Every function above, plus exactly the constants this file defines - see
# $PreConfigVariableNames near the top for why the list is computed and not
# spelled out.
Export-ModuleMember -Function * -Variable (
    (Get-Variable -Scope Script).Name |
        Where-Object { $PreConfigVariableNames -notcontains $_ }
)
