# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Backs up, overrides and restores the game's settings, for automated testing.

.DESCRIPTION
    Disco Elysium keeps its settings in its OWN file, not in Unity's PlayerPrefs:

        %USERPROFILE%\AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json

    SettingsPersister reads and writes it through JsonUtil, which resolves
    Application.persistentDataPath + "/Settings/Settings.json". That file is the
    one that decides what the game does.

    The Unity PlayerPrefs registry key HKCU\Software\ZAUM Studio\Disco Elysium is
    a downstream CACHE, not a second source of truth. Unity opens the window at
    the registry's resolution before any game code runs; then
    ResolutionSwitcher.Start reads the saved resolution out of Settings.json,
    Apply calls Screen.SetResolution, and Unity writes the result back into the
    registry. Verified by observation: with the two disagreeing before launch, the
    game used the JSON value and the registry afterwards matched it.

    So this module edits the JSON, and backs up the registry as well - a test run
    changes it as a side effect, and leaving it holding the test's resolution
    would make the next launch open its window at the wrong size before the game
    corrected itself.

    Resolution and full-screen state also have a documented override that touches
    nothing on disk - the Unity player arguments -screen-width, -screen-height,
    -screen-fullscreen and -monitor
    (https://docs.unity3d.com/Manual/PlayerCommandLineArguments.html), built by
    Get-GameLaunchArgument. There is no way to relocate either store: PlayerPrefs
    is pinned to Company\Product
    (https://docs.unity3d.com/2020.1/Documentation/ScriptReference/PlayerPrefs.html)
    and Application.persistentDataPath is read-only
    (https://docs.unity3d.com/ScriptReference/Application-persistentDataPath.html).

    Hardcore mode is NOT a setting. GameModePersister serialises it into the save
    file, so it is chosen at character creation and travels with the save.

.NOTES
    A resolution the display does not offer is not an error. ResolutionSwitcher
    looks the saved one up among Screen.resolutions and, failing to find it, falls
    through to the LARGEST compatible mode and writes that back - so an
    unsupported value silently becomes the monitor's maximum. Ask for one the
    display actually has.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# DISCO_ELYSIUM_GCT_SETTINGS_FILE redirects the settings file, which is how this
# module's own round-trip test runs against a scratch copy.
$script:DefaultSettingsFile = Join-Path $env:USERPROFILE `
    'AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json'
$script:SettingsFile = if ($env:DISCO_ELYSIUM_GCT_SETTINGS_FILE) {
    $env:DISCO_ELYSIUM_GCT_SETTINGS_FILE
} else {
    $script:DefaultSettingsFile
}

# The PlayerPrefs cache. Backed up, never treated as authoritative.
$script:RegistryKeyForReg = 'HKCU\Software\ZAUM Studio\Disco Elysium'
$script:RegistryKey = 'HKCU:\Software\ZAUM Studio\Disco Elysium'

# SettingsValue.type to the field carrying the value.
$script:FieldForType = @{
    'INT'    = 'intValue'
    'FLOAT'  = 'floatValue'
    'BOOL'   = 'boolValue'
    'STRING' = 'stringValue'
}

function Get-GameSettingsPath {
    <#
    .SYNOPSIS
        The settings file the game reads and writes.
    #>
    [CmdletBinding()]
    param()

    return $script:SettingsFile
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

function Read-GameSettings {
    <#
    .SYNOPSIS
        The settings file, parsed.
    #>
    [CmdletBinding()]
    param()

    if (-not (Test-GameSettingsPresent)) {
        throw "No settings file at $script:SettingsFile. Launch the game once first."
    }

    return Get-Content -LiteralPath $script:SettingsFile -Raw -Encoding UTF8 |
        ConvertFrom-Json
}

function Write-GameSettings {
    <#
    .SYNOPSIS
        Writes the settings file back.

    .DESCRIPTION
        Reformats the whole document, which is fine because the game only needs to
        parse it and because a restore is a byte-for-byte file copy rather than a
        re-serialisation. Depth is set well past the three levels the structure
        actually has: ConvertTo-Json defaults to 2 and would silently flatten the
        settings into strings.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)] $Settings
    )

    if ($PSCmdlet.ShouldProcess($script:SettingsFile, 'write settings')) {
        $json = $Settings | ConvertTo-Json -Depth 20
        Set-Content -LiteralPath $script:SettingsFile -Value $json -Encoding UTF8 -NoNewline
    }
}

function Resolve-GameSettingEntry {
    <#
    .SYNOPSIS
        Locates one setting, given "CATEGORY/name" or a bare name.

    .DESCRIPTION
        A bare name is searched across every category and refused if more than one
        matches, because quietly writing a same-named setting in the wrong category
        is worse than making the caller be specific.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] $Settings,
        [Parameter(Mandatory)] [string] $Name
    )

    if ($Name.Contains('/')) {
        $parts = $Name.Split('/', 2)
        $category = $parts[0]
        $leaf = $parts[1]

        if (-not $Settings.PSObject.Properties.Name.Contains($category)) {
            throw "No settings category '$category'. Categories: $($Settings.PSObject.Properties.Name -join ', ')."
        }
        if (-not $Settings.$category.PSObject.Properties.Name.Contains($leaf)) {
            throw "No setting '$leaf' in category '$category'."
        }

        return [pscustomobject]@{ Category = $category; Name = $leaf }
    }

    $found = @()
    foreach ($category in $Settings.PSObject.Properties.Name) {
        if ($Settings.$category.PSObject.Properties.Name.Contains($Name)) {
            $found += $category
        }
    }

    if ($found.Count -eq 0) {
        throw "No setting named '$Name' in any category."
    }
    if ($found.Count -gt 1) {
        throw "'$Name' exists in $($found.Count) categories: $($found -join ', '). Qualify it as CATEGORY/$Name."
    }

    return [pscustomobject]@{ Category = $found[0]; Name = $Name }
}

function Get-GameSetting {
    <#
    .SYNOPSIS
        One setting's current value, read from whichever field its type names.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Name,
        $Settings
    )

    if (-not $Settings) { $Settings = Read-GameSettings }
    $at = Resolve-GameSettingEntry -Settings $Settings -Name $Name
    $entry = $Settings.($at.Category).($at.Name)

    $field = $script:FieldForType[$entry.type]
    if (-not $field) {
        throw "Setting '$Name' has unrecognised type '$($entry.type)'."
    }

    return $entry.$field
}

function Set-GameSetting {
    <#
    .SYNOPSIS
        Overwrites one setting, in the field its declared type names.

    .DESCRIPTION
        Writes only the field the type points at, leaving the other three as the
        game left them. It does not invent settings: a name that is not already
        there is an error, because a setting the game has never written is one this
        build may not read.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] $Value,
        [Parameter(Mandatory)] $Settings
    )

    $at = Resolve-GameSettingEntry -Settings $Settings -Name $Name
    $entry = $Settings.($at.Category).($at.Name)

    $field = $script:FieldForType[$entry.type]
    if (-not $field) {
        throw "Setting '$Name' has unrecognised type '$($entry.type)'."
    }

    $entry.$field = $Value
    return $Settings
}

function Backup-GameSettings {
    <#
    .SYNOPSIS
        Copies the settings file, and exports the PlayerPrefs key beside it.

    .DESCRIPTION
        A file copy, so the backup is byte-for-byte and a restore cannot lose
        anything to a re-serialisation. The registry export is the cache, kept for
        the same run so a restore can put both back together.
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

function Get-GameLaunchArgument {
    <#
    .SYNOPSIS
        Unity player arguments for the display settings that have them.

    .DESCRIPTION
        These override without writing anything, so prefer them where they
        suffice. Note that the game applies its own saved resolution shortly after
        startup, so an argument can be overridden by Settings.json a moment later -
        set the JSON as well if the resolution has to stick.
    #>
    [CmdletBinding()]
    param(
        [int] $Width,
        [int] $Height,
        [switch] $Windowed,
        [switch] $FullScreen,
        [int] $Monitor = -1
    )

    if ($Windowed -and $FullScreen) {
        throw 'Pass -Windowed or -FullScreen, not both.'
    }

    $arguments = @()
    if ($Width -gt 0) { $arguments += @('-screen-width', $Width) }
    if ($Height -gt 0) { $arguments += @('-screen-height', $Height) }
    if ($Windowed) { $arguments += @('-screen-fullscreen', 0) }
    if ($FullScreen) { $arguments += @('-screen-fullscreen', 1) }
    if ($Monitor -ge 0) { $arguments += @('-monitor', $Monitor) }

    return $arguments
}

function Invoke-WithGameSettings {
    <#
    .SYNOPSIS
        Runs a script block with the game's settings temporarily overridden.

    .DESCRIPTION
        Backs up, applies, runs, and restores in a finally, so the settings come
        back even if the block throws or the run kills the game. A failed restore
        keeps the backup and says where it is, because the alternative is settings
        nobody can put back.

    .PARAMETER Settings
        Setting name to value. Name a setting "CATEGORY/name" - "GRAPHICS/resolutionWidth" -
        or by its bare name where that is unambiguous.

    .EXAMPLE
        Invoke-WithGameSettings -Settings @{
            'GRAPHICS/resolutionWidth'  = 1280
            'GRAPHICS/resolutionHeight' = 720
            'GRAPHICS/DISPLAY MODE'     = 1
        } -ScriptBlock {
            Start-Process -Wait $gameExe
        }
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [hashtable] $Settings,
        [Parameter(Mandatory)] [scriptblock] $ScriptBlock,
        [string] $BackupPath,
        [switch] $SkipRegistry
    )

    if (-not $BackupPath) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $BackupPath = Join-Path ([System.IO.Path]::GetTempPath()) "disco-settings-$stamp.json"
    }

    $backup = Backup-GameSettings -Path $BackupPath -SkipRegistry:$SkipRegistry
    Write-Verbose "Settings backed up to $($backup.SettingsPath)"

    $restored = $false
    try {
        $document = Read-GameSettings
        foreach ($name in $Settings.Keys) {
            $document = Set-GameSetting -Settings $document -Name $name -Value $Settings[$name]
            Write-Verbose "Set '$name' to $($Settings[$name])"
        }

        Write-GameSettings -Settings $document
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
    Test-GameSettingsPresent, `
    Read-GameSettings, `
    Write-GameSettings, `
    Resolve-GameSettingEntry, `
    Get-GameSetting, `
    Set-GameSetting, `
    Backup-GameSettings, `
    Restore-GameSettings, `
    Get-GameLaunchArgument, `
    Invoke-WithGameSettings
