<#
    Shared configuration and helpers for the five .ps1 scripts at the repo root.

    A MODULE, deliberately, and not a dot-sourced .ps1. PowerShell runs a
    dot-sourced script's param() block in the CALLER's scope, so a shared script
    that declares -DiscoElysiumDir silently reset its caller's $DiscoElysiumDir
    to $null the moment it was dot-sourced. That is de-3pw: build.ps1
    -DiscoElysiumDir <path> built happily against the auto-discovered Steam
    install instead, and deploy.ps1 / make-release.ps1 lost -Configuration the
    same way. Import-Module has no such scope leak - a module's contents are
    never executed in the importer's scope - so keeping every shared name in
    here makes that whole class of bug impossible rather than merely absent.

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
$AssemblyName = "UnifiedConversationTracker"
$TargetFramework = "net6.0"
$ProjectDir = Join-Path $RepoRoot "src\$AssemblyName.Plugin"
$ProjectFile = Join-Path $ProjectDir "$AssemblyName.Plugin.csproj"

# Everything generated lives under one gitignored folder: bin\ and obj\ are
# redirected here by Directory.Build.props, and these scripts add cache\, stage\
# and dist\ alongside them.
$BuildDir = Join-Path $RepoRoot ".build"
$CacheDir = Join-Path $BuildDir "cache"
$DistDir = Join-Path $BuildDir "dist"
# Must match BaseOutputPath in Directory.Build.props.
$BinDir = Join-Path $BuildDir "bin\$AssemblyName.Plugin"
# Caches the resolved reference install so repeat builds skip Steam discovery.
$RefDirCacheFile = Join-Path $CacheDir "reference-game-dir.txt"

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

# Read-only reference copies of the game kept in this repo. Builds must never
# write into them, and deploy refuses to target them unless explicitly forced.
# "Unaltered" means unaltered since the copy was taken, not unmodded: it carries
# the same BepInEx tree as the Steam install it was copied from. That is settled
# and accepted (de-omm.13, decided 2026-08-15); no pristine copy is kept.
$ReferenceCopyDirNames = @(
    "Steam Install - Unaltered"
)

# The csproj's own fallback, mirrored here so option 3 of the reference
# resolution order (see Resolve-ReferenceGameDir) matches it.
$RepoDefaultGameDir = Join-Path $RepoRoot "Steam Install - Unaltered\Disco Elysium"

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


function Resolve-ReferenceGameDir {
    # The game install the build reads its reference assemblies from. Resolution
    # order: explicit parameter, DISCO_ELYSIUM_DIR, the repo-local reference
    # copy, the cached previous answer, then Steam discovery. Read-only use, so
    # falling back to a discovered install is safe.
    param([string]$DiscoElysiumDir)

    if ($DiscoElysiumDir) {
        if (-not (Test-ReferenceGameDir -Path $DiscoElysiumDir)) {
            throw "-DiscoElysiumDir '$DiscoElysiumDir' has no $BepInExCoreRelDir + $BepInExInteropRelDir. Point it at a Disco Elysium install that has been run once with BepInEx 6."
        }
        return $DiscoElysiumDir
    }
    if ($env:DISCO_ELYSIUM_DIR) {
        if (-not (Test-ReferenceGameDir -Path $env:DISCO_ELYSIUM_DIR)) {
            throw "DISCO_ELYSIUM_DIR='$($env:DISCO_ELYSIUM_DIR)' has no $BepInExCoreRelDir + $BepInExInteropRelDir. Point it at a Disco Elysium install that has been run once with BepInEx 6."
        }
        return $env:DISCO_ELYSIUM_DIR
    }
    if (Test-ReferenceGameDir -Path $RepoDefaultGameDir) {
        return $RepoDefaultGameDir
    }
    if (Test-Path -LiteralPath $RefDirCacheFile) {
        $cached = (Get-Content -LiteralPath $RefDirCacheFile -Raw).Trim()
        if (Test-ReferenceGameDir -Path $cached) { return $cached }
    }

    $steamDir = Find-SteamGameDir
    if ($steamDir -and (Test-ReferenceGameDir -Path $steamDir)) {
        New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
        Set-Content -LiteralPath $RefDirCacheFile -Value $steamDir -Encoding ascii
        return $steamDir
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
  3. $RepoDefaultGameDir
  4. Steam auto-discovery
$steamNote
Pass -DiscoElysiumDir <path> or set DISCO_ELYSIUM_DIR to an install that has
both $BepInExCoreRelDir and $BepInExInteropRelDir.
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

    Write-Host "Building $AssemblyName v$(Get-PluginVersion) ($Configuration)..."
    dotnet build $ProjectFile -c $Configuration "-p:DiscoElysiumDir=$gameDir" | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet build failed with exit code $LASTEXITCODE"
    }

    $dllPath = Join-Path $BinDir "$Configuration\$TargetFramework\$AssemblyName.dll"
    if (-not (Test-Path -LiteralPath $dllPath)) {
        throw "Expected build output not found: $dllPath"
    }
    return $dllPath
}


function Copy-PluginPayload {
    # Copy everything that makes up an installed plugin into $DestDir, creating it
    # if needed. Shared by deploy.ps1 and make-release.ps1 so an installed copy and
    # a packaged copy always hold the same files.
    #
    # That is the plugin DLL plus the mod's own library assemblies next to it
    # (Core, Persistence, Session), each with its .pdb if one was produced so
    # exception stack traces carry line numbers. BepInEx resolves a plugin's
    # dependencies out of the plugin's own folder, so shipping the DLL alone would
    # load and then fail the moment it touched the unified state.
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
    $files = @(Get-ChildItem -LiteralPath $buildDir -File |
        Where-Object { $_.Name -like "$AssemblyName*" -and $_.Extension -in ".dll", ".pdb" } |
        Sort-Object Name)
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
