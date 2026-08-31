# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Swaps in a fixed test settings file for the duration of a script block, and
    puts the player's own settings back afterwards.

.DESCRIPTION
    Disco Elysium keeps its settings in its OWN file, not in Unity's PlayerPrefs:

        %USERPROFILE%\AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json

    SettingsPersister reads and writes it through JsonUtil, which resolves
    Application.persistentDataPath + "/Settings/Settings.json". That file decides
    what the game does.

    The Unity PlayerPrefs registry key HKCU\Software\ZAUM Studio\Disco Elysium is
    a downstream CACHE, not a second source of truth. Unity opens the window at
    the registry's resolution before any game code runs; then ResolutionSwitcher
    reads the saved resolution out of the JSON, applies it, and Unity writes the
    result back into the registry. Verified by observation: with the two
    disagreeing before launch, the game used the JSON and the registry afterwards
    matched it.

    So a test run installs testing/Settings.json wholesale rather than editing the
    player's file in place. A whole file is reproducible - every run starts from
    exactly the same settings, whatever the player last chose - and it removes the
    editing code and its failure modes entirely.

    The registry is backed up and restored too, because a run changes it as a side
    effect: left holding 1280x720, the next launch would open its window at that
    size before the game corrected itself.

.NOTES
    A resolution the display does not offer is not an error. ResolutionSwitcher
    looks the saved one up among Screen.resolutions and, failing to find it, falls
    through to the LARGEST compatible mode and writes that back. 1280x720 is the
    smallest the game will ever offer - GetCompatibleResolutions filters to
    width >= 1280 - and is near-universally supported, but a display that lacks it
    will silently get its maximum instead.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# DISCO_ELYSIUM_GCT_SETTINGS_FILE redirects the live settings file, which is how
# this module's own round-trip test runs against a scratch copy.
$script:DefaultSettingsFile = Join-Path $env:USERPROFILE `
    'AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json'
$script:SettingsFile = if ($env:DISCO_ELYSIUM_GCT_SETTINGS_FILE) {
    $env:DISCO_ELYSIUM_GCT_SETTINGS_FILE
} else {
    $script:DefaultSettingsFile
}

# The settings every test run uses: 1280x720, windowed, cheapest rendering,
# silent, no tutorial. See testing/Settings.json for the values and why.
$script:TestSettingsFile = Join-Path $PSScriptRoot 'testing\Settings.json'

# The PlayerPrefs cache. Backed up, never treated as authoritative.
$script:RegistryKeyForReg = 'HKCU\Software\ZAUM Studio\Disco Elysium'
$script:RegistryKey = 'HKCU:\Software\ZAUM Studio\Disco Elysium'

function Get-GameSettingsPath {
    <#
    .SYNOPSIS
        The settings file the game reads and writes.
    #>
    [CmdletBinding()]
    param()

    return $script:SettingsFile
}

function Get-TestSettingsPath {
    <#
    .SYNOPSIS
        The fixed settings file installed for a test run.
    #>
    [CmdletBinding()]
    param()

    return $script:TestSettingsFile
}

function Test-GameSettingsPresent {
    <#
    .SYNOPSIS
        Whether the settings file exists. It does not until the game has run once.
    #>
    [CmdletBinding()]
    param()

    return Test-Path -LiteralPath $script:SettingsFile
}

function Get-GameSetting {
    <#
    .SYNOPSIS
        One setting's value, read from whichever field its declared type names.

    .DESCRIPTION
        For inspecting and asserting. Nothing here writes settings - a test run
        installs a whole file instead.

    .PARAMETER Name
        "CATEGORY/name", as in "GRAPHICS/resolutionWidth".
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Name,
        [string] $Path
    )

    if (-not $Path) { $Path = $script:SettingsFile }
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "No settings file at $Path."
    }

    $parts = $Name.Split('/', 2)
    if ($parts.Count -ne 2) {
        throw "Name a setting as CATEGORY/name, for example GRAPHICS/resolutionWidth."
    }

    $settings = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not $settings.PSObject.Properties.Name.Contains($parts[0])) {
        throw "No settings category '$($parts[0])' in $Path."
    }
    if (-not $settings.($parts[0]).PSObject.Properties.Name.Contains($parts[1])) {
        throw "No setting '$($parts[1])' in category '$($parts[0])'."
    }

    $entry = $settings.($parts[0]).($parts[1])
    switch ($entry.type) {
        'INT'    { return $entry.intValue }
        'FLOAT'  { return $entry.floatValue }
        'BOOL'   { return $entry.boolValue }
        'STRING' { return $entry.stringValue }
        default  { throw "Setting '$Name' has unrecognised type '$($entry.type)'." }
    }
}

function Backup-GameSettings {
    <#
    .SYNOPSIS
        Copies the settings file, and exports the PlayerPrefs key beside it.

    .DESCRIPTION
        A file copy, so the backup is byte-for-byte and a restore cannot lose
        anything to re-serialisation.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Path,
        [switch] $SkipRegistry
    )

    if (-not (Test-GameSettingsPresent)) {
        throw "Nothing to back up: no settings file at $script:SettingsFile."
    }

    $directory = Split-Path -Parent $Path
    if ($directory -and -not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    Copy-Item -LiteralPath $script:SettingsFile -Destination $Path -Force

    $registryPath = $null
    if (-not $SkipRegistry -and (Test-Path -LiteralPath $script:RegistryKey)) {
        $registryPath = "$Path.reg"
        & reg.exe export $script:RegistryKeyForReg $registryPath /y 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "reg export of $script:RegistryKeyForReg failed with exit code $LASTEXITCODE."
        }
    }

    return [pscustomobject]@{
        SettingsPath = (Resolve-Path -LiteralPath $Path).Path
        RegistryPath = $registryPath
    }
}

function Restore-GameSettings {
    <#
    .SYNOPSIS
        Puts the settings file, and the PlayerPrefs cache, back as they were.

    .DESCRIPTION
        The registry is deleted before importing, because an import alone merges:
        a value the run added would survive, and the restore would be a lie.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)] $Backup
    )

    if (-not (Test-Path -LiteralPath $Backup.SettingsPath)) {
        throw "No settings backup at $($Backup.SettingsPath)."
    }

    if (-not $PSCmdlet.ShouldProcess($script:SettingsFile, 'restore from backup')) {
        return
    }

    Copy-Item -LiteralPath $Backup.SettingsPath -Destination $script:SettingsFile -Force

    if ($Backup.RegistryPath -and (Test-Path -LiteralPath $Backup.RegistryPath)) {
        if (Test-Path -LiteralPath $script:RegistryKey) {
            & reg.exe delete $script:RegistryKeyForReg /f 2>&1 | Out-Null
        }

        & reg.exe import $Backup.RegistryPath 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "reg import of $($Backup.RegistryPath) failed with exit code $LASTEXITCODE. " +
                  "The PlayerPrefs cache may be missing; the export is still there to import by hand."
        }
    }
}

function Invoke-WithTestSettings {
    <#
    .SYNOPSIS
        Runs a script block with the fixed test settings installed.

    .DESCRIPTION
        Backs up, installs testing/Settings.json, runs, and restores in a finally,
        so the player's settings come back even if the block throws or the run
        kills the game. A failed restore keeps the backup and says where it is,
        because the alternative is settings nobody can put back.

    .EXAMPLE
        Invoke-WithTestSettings { Start-Process -Wait $gameExe }
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory, Position = 0)] [scriptblock] $ScriptBlock,
        [string] $BackupPath,
        [string] $TestSettingsPath,
        [switch] $SkipRegistry
    )

    if (-not $TestSettingsPath) { $TestSettingsPath = $script:TestSettingsFile }
    if (-not (Test-Path -LiteralPath $TestSettingsPath)) {
        throw "No test settings file at $TestSettingsPath."
    }

    if (-not $BackupPath) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $BackupPath = Join-Path ([System.IO.Path]::GetTempPath()) "disco-settings-$stamp.json"
    }

    $backup = Backup-GameSettings -Path $BackupPath -SkipRegistry:$SkipRegistry
    Write-Verbose "Settings backed up to $($backup.SettingsPath)"

    $restored = $false
    try {
        Copy-Item -LiteralPath $TestSettingsPath -Destination $script:SettingsFile -Force
        Write-Verbose "Installed test settings from $TestSettingsPath"

        & $ScriptBlock
    }
    finally {
        try {
            Restore-GameSettings -Backup $backup
            $restored = $true
            Write-Verbose 'Settings restored.'
        }
        catch {
            Write-Warning "Could not restore the game's settings: $_"
            Write-Warning "The backup is at $($backup.SettingsPath) - copy it back by hand."
        }
    }

    if ($restored) {
        Remove-Item -LiteralPath $backup.SettingsPath -ErrorAction SilentlyContinue
        if ($backup.RegistryPath) {
            Remove-Item -LiteralPath $backup.RegistryPath -ErrorAction SilentlyContinue
        }
    }
}

Export-ModuleMember -Function `
    Get-GameSettingsPath, `
    Get-TestSettingsPath, `
    Test-GameSettingsPresent, `
    Get-GameSetting, `
    Backup-GameSettings, `
    Restore-GameSettings, `
    Invoke-WithTestSettings
