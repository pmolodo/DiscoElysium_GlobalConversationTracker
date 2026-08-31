# SPDX-License-Identifier: MIT
<#
    Exercises game-settings.psm1 against a scratch copy of a settings file, never
    the real one, and with the registry left alone entirely.

    Uses the real Settings.json as its fixture when there is one, because a
    hand-written stand-in would only prove the module works on the shape someone
    imagined. Falls back to a synthetic file so the test still runs on a machine
    with no installation.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scratchDir = Join-Path ([System.IO.Path]::GetTempPath()) "gct-settings-test-$PID"
$scratchFile = Join-Path $scratchDir 'Settings.json'
New-Item -ItemType Directory -Path $scratchDir -Force | Out-Null

$realFile = Join-Path $env:USERPROFILE `
    'AppData\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json'

if (Test-Path -LiteralPath $realFile) {
    Copy-Item -LiteralPath $realFile -Destination $scratchFile
    Write-Host "fixture: a copy of the real settings file"
} else {
    @'
{
  "GRAPHICS": {
    "resolutionWidth":  { "intValue": 1920, "stringValue": null, "floatValue": 0.0, "boolValue": false, "type": "INT" },
    "resolutionHeight": { "intValue": 1080, "stringValue": null, "floatValue": 0.0, "boolValue": false, "type": "INT" },
    "DISPLAY MODE":     { "intValue": 1,    "stringValue": null, "floatValue": 0.0, "boolValue": false, "type": "INT" },
    "BRIGHTNESS":       { "intValue": 0,    "stringValue": null, "floatValue": 400.0, "boolValue": false, "type": "FLOAT" },
    "detectiveMode":    { "intValue": 0,    "stringValue": null, "floatValue": 0.0, "boolValue": true,  "type": "BOOL" }
  },
  "LANGUAGE": {
    "CURRENT": { "intValue": 0, "stringValue": "English", "floatValue": 0.0, "boolValue": false, "type": "STRING" }
  }
}
'@ | Set-Content -LiteralPath $scratchFile -Encoding UTF8
    Write-Host "fixture: synthetic (no installation found)"
}

$env:DISCO_ELYSIUM_GCT_SETTINGS_FILE = $scratchFile
Import-Module (Join-Path $PSScriptRoot 'game-settings.psm1') -Force

$failures = @()
function Check($label, $expected, $actual) {
    if ($expected -eq $actual) {
        Write-Host "  PASS  $label"
    } else {
        Write-Host "  FAIL  $label (expected '$expected', got '$actual')"
        $script:failures += $label
    }
}

Write-Host "`ntargeting: $(Get-GameSettingsPath)"
Check 'module targets the scratch file' $scratchFile (Get-GameSettingsPath)
Check 'settings are present' $true (Test-GameSettingsPresent)

$originalWidth = Get-GameSetting -Name 'GRAPHICS/resolutionWidth'
$originalBrightness = Get-GameSetting -Name 'GRAPHICS/BRIGHTNESS'
$originalDetective = Get-GameSetting -Name 'GRAPHICS/detectiveMode'
$originalBytes = [System.IO.File]::ReadAllBytes($scratchFile)

Write-Host "`nreading, by type:"
Write-Host "  width=$originalWidth brightness=$originalBrightness detectiveMode=$originalDetective"
Check 'an INT reads from intValue' $true ($originalWidth -is [int])
Check 'a FLOAT reads from floatValue' $true ($originalBrightness -is [double] -or $originalBrightness -is [decimal])
Check 'a BOOL reads from boolValue' $true ($originalDetective -is [bool])

Write-Host "`nbare names resolve, ambiguous ones are refused:"
Check 'bare name finds its category' $originalWidth (Get-GameSetting -Name 'resolutionWidth')
$threw = $false
try { Get-GameSetting -Name 'NoSuchSettingAnywhere' } catch { $threw = $true }
Check 'an unknown name throws' $true $threw

Write-Host "`nround trip (registry untouched):"
$ran = $false
Invoke-WithGameSettings -SkipRegistry -Settings @{
    'GRAPHICS/resolutionWidth'  = 1280
    'GRAPHICS/resolutionHeight' = 720
} -ScriptBlock {
    $script:ran = $true
    Check 'override visible inside the block' 1280 (Get-GameSetting -Name 'GRAPHICS/resolutionWidth')
    Check 'second override visible too' 720 (Get-GameSetting -Name 'GRAPHICS/resolutionHeight')
    Check 'an untouched setting is unchanged' $originalBrightness `
        (Get-GameSetting -Name 'GRAPHICS/BRIGHTNESS')
}

Check 'the script block ran' $true $ran
Check 'width restored' $originalWidth (Get-GameSetting -Name 'GRAPHICS/resolutionWidth')

$restoredBytes = [System.IO.File]::ReadAllBytes($scratchFile)
Check 'the file is restored byte for byte' `
    ([Convert]::ToBase64String($originalBytes)) ([Convert]::ToBase64String($restoredBytes))

Write-Host "`nrestores even when the block throws:"
try {
    Invoke-WithGameSettings -SkipRegistry -Settings @{ 'GRAPHICS/resolutionWidth' = 640 } -ScriptBlock {
        throw 'the game crashed'
    }
} catch {
    Write-Host "  (caught: $($_.Exception.Message))"
}
Check 'width restored after a throw' $originalWidth (Get-GameSetting -Name 'GRAPHICS/resolutionWidth')
Check 'still byte for byte after a throw' `
    ([Convert]::ToBase64String($originalBytes)) `
    ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($scratchFile)))

Write-Host "`nthe game can still parse what we write:"
Invoke-WithGameSettings -SkipRegistry -Settings @{ 'GRAPHICS/resolutionWidth' = 1600 } -ScriptBlock {
    $reparsed = Get-Content -LiteralPath $scratchFile -Raw -Encoding UTF8 | ConvertFrom-Json
    Check 'the written file parses' 1600 $reparsed.GRAPHICS.resolutionWidth.intValue
    Check 'every category survives the rewrite' `
        (@((Get-Content -LiteralPath $scratchFile -Raw | ConvertFrom-Json).PSObject.Properties.Name).Count) `
        (@($reparsed.PSObject.Properties.Name).Count)
}

Write-Host "`nlaunch arguments:"
Check 'windowed 1280x720' '-screen-width 1280 -screen-height 720 -screen-fullscreen 0' `
    ((Get-GameLaunchArgument -Width 1280 -Height 720 -Windowed) -join ' ')

Remove-Item -LiteralPath $scratchDir -Recurse -Force
Remove-Item Env:\DISCO_ELYSIUM_GCT_SETTINGS_FILE

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host 'ALL PASS'
} else {
    Write-Host "FAILURES: $($failures -join ', ')"
    exit 1
}
