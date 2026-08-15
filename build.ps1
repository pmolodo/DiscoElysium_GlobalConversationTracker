#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Builds UnifiedConversationTracker.dll, the BepInEx plugin.

.DESCRIPTION
    Resolves and verifies the reference assemblies (via provision-refs.ps1),
    then runs `dotnet build` on src\UnifiedConversationTracker.Plugin, handing
    the resolved game directory to the csproj as -p:DiscoElysiumDir so the
    project's own reference-resolution mechanism is what actually runs.

    Nothing is written outside the repo: build output lands in the gitignored
    .build\ folder (see Directory.Build.props). The game install is only read
    from.

    Dot-source this file to reuse Invoke-PluginBuild and, transitively,
    provision-refs.ps1's constants and functions; run it directly to just build.
#>
[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$DiscoElysiumDir
)

$ErrorActionPreference = "Stop"

# Reference resolution + shared project config. Dot-sourced so its variables and
# functions ($ProjectFile, $AssemblyName, Initialize-BuildReferences, ...) are
# available here and to anything that dot-sources this script.
. (Join-Path $PSScriptRoot "provision-refs.ps1")


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


# Build when invoked directly (not when dot-sourced for reuse).
if ($MyInvocation.InvocationName -ne '.') {
    Invoke-ScriptMain {
        $dll = Invoke-PluginBuild -Configuration $Configuration -DiscoElysiumDir $DiscoElysiumDir
        Write-Host "Built: $dll" -ForegroundColor Green
    }
}
