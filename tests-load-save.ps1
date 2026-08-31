#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Launches the game, waits for the main menu, and loads a save - by screenshot
    and keyboard alone.

.DESCRIPTION
    The first end-to-end harness. It deliberately learns nothing from inside the
    game: loading is detected by watching the screen settle, not by a hook.

    Two passes are needed the first time. Run with -CaptureReference to launch,
    wait for the screen to settle, and save what it saw as the main-menu
    reference; look at the PNG to confirm it really is the menu. After that, plain
    runs wait for the screen to match that reference before sending any keys,
    which is what stops a slow load from typing into a loading screen.

    The key sequence is a parameter rather than a constant because a menu's
    keyboard navigation is not something to guess at: run it once, watch, and
    adjust. -DryRun prints the sequence and captures screenshots without pressing
    anything.

.EXAMPLE
    .\tests-load-save.ps1 -CaptureReference
    .\tests-load-save.ps1 -Verbose
#>

[CmdletBinding()]
param(
    [string] $GamePath,
    [string] $ProcessName = 'disco',

    # Where the reference image and this run's screenshots go. Defaulted in the
    # body, not here: $PSScriptRoot is not populated while param() defaults are
    # evaluated, so a default built from it silently comes out empty.
    [string] $ArtifactDirectory,

    # Captures the main-menu reference instead of asserting against it.
    [switch] $CaptureReference,

    # Goes through the motions without sending a keystroke.
    [switch] $DryRun,

    # How the main menu is navigated to a loaded save. Verify before trusting.
    [string[]] $LoadSaveKeys = @('Down', 'Enter', 'Enter'),

    [double] $MenuThreshold = 0.05,
    [int] $LaunchTimeoutSeconds = 300,
    [int] $LoadTimeoutSeconds = 300,

    # Leaves the game running, for looking at what happened.
    [switch] $KeepOpen
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Import-Module (Join-Path $PSScriptRoot 'game-automation.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'game-settings.psm1') -Force

if (-not $ArtifactDirectory) {
    $ArtifactDirectory = Join-Path $PSScriptRoot '.build\automation'
}

$referencePath = Join-Path $ArtifactDirectory 'main-menu.png'
if (-not (Test-Path -LiteralPath $ArtifactDirectory)) {
    New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
}

function Resolve-GameExecutable {
    if ($GamePath) {
        if (-not (Test-Path -LiteralPath $GamePath)) { throw "No game at $GamePath." }
        return $GamePath
    }

    # The same Steam library the build scripts resolve against.
    $candidates = @(
        'C:\apps (x86)\games\steam\steamapps\common\Disco Elysium\disco.exe',
        'C:\Program Files (x86)\Steam\steamapps\common\Disco Elysium\disco.exe'
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }

    throw 'Could not find disco.exe. Pass -GamePath.'
}

$failures = @()
function Check($label, $condition, $detail = '') {
    if ($condition) {
        Write-Host "  PASS  $label $detail"
    } else {
        Write-Host "  FAIL  $label $detail"
        $script:failures += $label
    }
}

$exe = Resolve-GameExecutable
Write-Host "game:       $exe"
Write-Host "artifacts:  $ArtifactDirectory"
Write-Host "settings:   $(Get-TestSettingsPath)"
if ($DryRun) { Write-Host 'MODE:       dry run - no keys will be sent' }

if (Get-Process -Name $ProcessName -ErrorAction SilentlyContinue) {
    throw "'$ProcessName' is already running. Close it first; two windows would make the capture ambiguous."
}

$process = $null

Invoke-WithTestSettings {
    Write-Host "`nlaunching..."
    $script:process = Start-Process -FilePath $exe -PassThru

    $window = Wait-GameWindow -ProcessName $ProcessName -TimeoutSeconds $LaunchTimeoutSeconds
    Write-Host "  window: '$($window.Title)'"
    Check 'the game window appeared' $true

    $foreground = Set-GameWindowForeground -Window $window
    Check 'the window came to the front' $foreground `
        $(if (-not $foreground) { '(capture and input both need this - is something else stealing focus?)' } else { '' })

    $rect = Get-GameWindowRect -Window $window
    Write-Host "  client area: $($rect.Width)x$($rect.Height)"
    Check 'the test settings resolution took effect' `
        ($rect.Width -eq 1280 -and $rect.Height -eq 720) `
        "(got $($rect.Width)x$($rect.Height); the display may not offer 1280x720, in which case the game snapped to its maximum)"

    Write-Host "`nwaiting for the screen to settle..."
    $settled = Wait-GameScreenStable -Window $window -TimeoutSeconds $LaunchTimeoutSeconds -Verbose:$VerbosePreference
    Check 'the screen stopped changing' $settled.Settled `
        ("(difference {0:N4} after {1}s)" -f $settled.Difference, $settled.Waited)

    $shot = Get-GameScreenshot -Window $window -Path (Join-Path $ArtifactDirectory 'after-launch.png')
    $shot.Dispose()

    if ($CaptureReference) {
        Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'after-launch.png') `
            -Destination $referencePath -Force
        Write-Host "`nSaved the main-menu reference to $referencePath"
        Write-Host 'Look at it and confirm it is the main menu before relying on it.'
        return
    }

    if (-not (Test-Path -LiteralPath $referencePath)) {
        throw "No main-menu reference at $referencePath. Run once with -CaptureReference first."
    }

    Write-Host "`nchecking we are at the main menu..."
    $atMenu = Wait-GameScreen -Window $window -ReferencePath $referencePath `
        -Threshold $MenuThreshold -TimeoutSeconds 30 -Verbose:$VerbosePreference
    Check 'the main menu is on screen' $atMenu.Matched `
        ("(closest difference {0:N4}, threshold {1:N4})" -f $atMenu.Best, $MenuThreshold)

    if (-not $atMenu.Matched) {
        Write-Host '  (see after-launch.png - if that IS the menu, raise -MenuThreshold)'
        return
    }

    Write-Host "`nloading a save: $($LoadSaveKeys -join ' -> ')"
    if ($DryRun) {
        Write-Host '  (dry run - not sent)'
        return
    }

    [void](Set-GameWindowForeground -Window $window)
    Send-GameKeys $LoadSaveKeys -Verbose:$VerbosePreference

    Write-Host "`nwaiting for the load to finish..."
    $loaded = Wait-GameScreenStable -Window $window -TimeoutSeconds $LoadTimeoutSeconds `
        -StableSamples 6 -Verbose:$VerbosePreference
    Check 'the screen settled again after loading' $loaded.Settled `
        ("(difference {0:N4})" -f $loaded.Difference)

    $shot = Get-GameScreenshot -Window $window -Path (Join-Path $ArtifactDirectory 'after-load.png')
    $shot.Dispose()

    # The menu and the loaded game must not look the same, or the keys did nothing
    # and the "settle" was just the menu sitting there.
    $stillMenu = Wait-GameScreen -Window $window -ReferencePath $referencePath `
        -Threshold $MenuThreshold -TimeoutSeconds 2
    Check 'the screen is no longer the main menu' (-not $stillMenu.Matched) `
        ("(difference from the menu {0:N4}; too low means the keys did nothing)" -f $stillMenu.Best)

    Write-Host "`nscreenshots are in $ArtifactDirectory"
}

if ($script:process -and -not $KeepOpen) {
    Write-Host "`nclosing the game..."
    try {
        $script:process | Stop-Process -Force -ErrorAction Stop
        $script:process.WaitForExit(15000) | Out-Null
    } catch {
        Write-Warning "Could not close the game: $_"
    }
}

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host 'ALL PASS'
} else {
    Write-Host "FAILURES: $($failures -join ', ')"
    exit 1
}
