# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Backs up, overrides and restores the game's settings, for automated testing.

.DESCRIPTION
    Disco Elysium keeps every display setting in Unity's PlayerPrefs, which on
    Windows is the registry key HKCU\Software\ZAUM Studio\Disco Elysium. There is
    no settings file, and no way to point Unity at a different PlayerPrefs
    location: the path is Company\Product and that is that
    (https://docs.unity3d.com/2020.1/Documentation/ScriptReference/PlayerPrefs.html).
    Application.persistentDataPath is read-only and has no environment override
    either (https://docs.unity3d.com/ScriptReference/Application-persistentDataPath.html).

    Resolution and full-screen state CAN be overridden without touching the
    registry, through the standard Unity player arguments -screen-width,
    -screen-height, -screen-fullscreen and -monitor
    (https://docs.unity3d.com/Manual/PlayerCommandLineArguments.html), which
    Get-GameLaunchArgument builds. Prefer those where they suffice: an argument
    changes nothing on disk.

    Everything else needs the backup-alter-restore closure below. Take the backup
    even when launching with arguments: Unity writes the resolution it ends up
    using back into PlayerPrefs, so an argument-only run can still leave the
    registry changed.

    Hardcore mode is NOT here, because it is not a setting. GameModePersister
    serialises it into the save file, so it is chosen at character creation and
    travels with the save; pick it by choosing which save to load.

.NOTES
    PlayerPrefs names carry a hash suffix - "Screenmanager Fullscreen mode" is
    stored as "Screenmanager Fullscreen mode_h3630240806". Computing that hash is
    avoided entirely by matching on the prefix, which works for any setting the
    game has already written at least once. A setting that has never been written
    has no value to match and cannot be set this way.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# The Unity company and product names, which together locate PlayerPrefs.
# DISCO_ELYSIUM_GCT_SETTINGS_KEY redirects them, which is what lets this module's
# own round-trip test run against a scratch key instead of a real installation.
# It takes the reg.exe form, without the 'HKCU:' drive colon.
$script:DefaultKeyForReg = 'HKCU\Software\ZAUM Studio\Disco Elysium'
$script:SettingsKeyForReg = if ($env:DISCO_ELYSIUM_GCT_SETTINGS_KEY) {
    $env:DISCO_ELYSIUM_GCT_SETTINGS_KEY
} else {
    $script:DefaultKeyForReg
}
if (-not $script:SettingsKeyForReg.StartsWith('HKCU\')) {
    throw "DISCO_ELYSIUM_GCT_SETTINGS_KEY must start with 'HKCU\', got '$script:SettingsKeyForReg'."
}

# The same key in PowerShell's provider form, which wants a drive colon.
$script:SettingsKey = 'HKCU:' + $script:SettingsKeyForReg.Substring('HKCU'.Length)

function Get-GameSettingsKeyPath {
    <#
    .SYNOPSIS
        The registry key holding the game's PlayerPrefs.
    #>
    [CmdletBinding()]
    param([switch] $ForRegExe)

    if ($ForRegExe) { return $script:SettingsKeyForReg }
    return $script:SettingsKey
}

function Test-GameSettingsPresent {
    <#
    .SYNOPSIS
        Whether the game has written its settings at least once.
    #>
    [CmdletBinding()]
    param()

    return Test-Path -LiteralPath $script:SettingsKey
}

function Resolve-GameSettingName {
    <#
    .SYNOPSIS
        The stored name of a PlayerPref, hash suffix included.

    .DESCRIPTION
        Matches on the prefix, so callers name settings the way Unity does
        ("Screenmanager Fullscreen mode") without knowing the hash. Fails loudly
        on an ambiguous prefix rather than picking one, because silently writing
        the wrong setting is worse than not writing it.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Name
    )

    if (-not (Test-GameSettingsPresent)) {
        throw "The game has no settings yet at $script:SettingsKey. Launch it once first."
    }

    $key = Get-Item -LiteralPath $script:SettingsKey
    $matches = @($key.GetValueNames() | Where-Object { $_ -eq $Name -or $_.StartsWith("$Name`_h") })

    if ($matches.Count -eq 0) {
        throw "No setting named '$Name' under $script:SettingsKey. It may never have been written."
    }
    if ($matches.Count -gt 1) {
        throw "'$Name' matches $($matches.Count) settings: $($matches -join ', ')."
    }

    return $matches[0]
}

function Get-GameSetting {
    <#
    .SYNOPSIS
        The current value of one setting.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Name
    )

    $stored = Resolve-GameSettingName -Name $Name
    return (Get-ItemProperty -LiteralPath $script:SettingsKey -Name $stored).$stored
}

function Set-GameSetting {
    <#
    .SYNOPSIS
        Overwrites one setting in place, keeping its stored name and type.

    .DESCRIPTION
        Only ever writes over a value that already exists, which is what lets it
        reuse the hashed name rather than compute one.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] $Value
    )

    $stored = Resolve-GameSettingName -Name $Name
    if ($PSCmdlet.ShouldProcess("$script:SettingsKey\$stored", "set to $Value")) {
        Set-ItemProperty -LiteralPath $script:SettingsKey -Name $stored -Value $Value
    }
}

function Backup-GameSettings {
    <#
    .SYNOPSIS
        Exports the settings key to a .reg file.

    .DESCRIPTION
        reg.exe rather than a PowerShell walk of the key, because the export
        round-trips every value type faithfully - including the REG_BINARY input
        bindings, which are large and which nobody wants to reconstruct by hand.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $Path
    )

    if (-not (Test-GameSettingsPresent)) {
        throw "Nothing to back up: $script:SettingsKey does not exist."
    }

    $directory = Split-Path -Parent $Path
    if ($directory -and -not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    & reg.exe export $script:SettingsKeyForReg $Path /y 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "reg export of $script:SettingsKeyForReg failed with exit code $LASTEXITCODE."
    }

    return (Resolve-Path -LiteralPath $Path).Path
}

function Restore-GameSettings {
    <#
    .SYNOPSIS
        Puts the settings key back exactly as a backup found it.

    .DESCRIPTION
        Deletes before importing, so a value the test ADDED is removed rather than
        left behind. An import alone merges, which would quietly leave new
        settings in place and make the restore a lie.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)] [string] $Path
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "No backup at $Path."
    }

    if (-not $PSCmdlet.ShouldProcess($script:SettingsKeyForReg, 'restore from backup')) {
        return
    }

    if (Test-GameSettingsPresent) {
        & reg.exe delete $script:SettingsKeyForReg /f 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "reg delete of $script:SettingsKeyForReg failed with exit code $LASTEXITCODE."
        }
    }

    & reg.exe import $Path 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "reg import of $Path failed with exit code $LASTEXITCODE. The game's settings may " +
              "be missing; the backup is still at $Path and can be imported by hand."
    }
}

function Get-GameLaunchArgument {
    <#
    .SYNOPSIS
        Unity player arguments for the display settings that have them.

    .DESCRIPTION
        These override without writing anything, so they are the better tool where
        they apply. See
        https://docs.unity3d.com/Manual/PlayerCommandLineArguments.html.

        Note that Unity persists the resolution it settles on, so a run launched
        this way can still leave PlayerPrefs changed - back up anyway.
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
        Backs up, applies the overrides, runs the block, and restores in a finally
        so the settings come back even if the block throws or the test kills the
        game. The backup file is kept on failure and its path reported, because a
        restore that itself fails must leave something a person can import by hand.

    .PARAMETER Settings
        Setting name to value, named as Unity does - "Screenmanager Fullscreen mode",
        "Screenmanager Resolution Width". The hash suffix is resolved for you.

    .EXAMPLE
        Invoke-WithGameSettings -Settings @{
            'Screenmanager Fullscreen mode'    = 3
            'Screenmanager Resolution Width'   = 1280
            'Screenmanager Resolution Height'  = 720
            'Screenmanager Resolution Use Native' = 0
        } -ScriptBlock {
            Start-Process -Wait $gameExe
        }
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [hashtable] $Settings,
        [Parameter(Mandatory)] [scriptblock] $ScriptBlock,
        [string] $BackupPath
    )

    if (-not $BackupPath) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $BackupPath = Join-Path ([System.IO.Path]::GetTempPath()) "disco-settings-$stamp.reg"
    }

    $saved = Backup-GameSettings -Path $BackupPath
    Write-Verbose "Game settings backed up to $saved"

    $restored = $false
    try {
        foreach ($name in $Settings.Keys) {
            Set-GameSetting -Name $name -Value $Settings[$name]
            Write-Verbose "Set '$name' to $($Settings[$name])"
        }

        & $ScriptBlock
    }
    finally {
        try {
            Restore-GameSettings -Path $saved
            $restored = $true
            Write-Verbose 'Game settings restored.'
        }
        catch {
            Write-Warning "Could not restore the game's settings: $_"
            Write-Warning "The backup is at $saved - import it to put them back."
        }
    }

    if ($restored) {
        Remove-Item -LiteralPath $saved -ErrorAction SilentlyContinue
    }
}

Export-ModuleMember -Function `
    Get-GameSettingsKeyPath, `
    Test-GameSettingsPresent, `
    Resolve-GameSettingName, `
    Get-GameSetting, `
    Set-GameSetting, `
    Backup-GameSettings, `
    Restore-GameSettings, `
    Get-GameLaunchArgument, `
    Invoke-WithGameSettings
