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
    Write-Host 'fixture: a copy of the real settings file'
} else {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'testing\Settings.json') `
        -Destination $scratchFile
    Write-Host 'fixture: the test settings file (no installation found)'
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

$originalBytes = [System.IO.File]::ReadAllBytes($scratchFile)
$originalWidth = Get-GameSetting -Name 'GRAPHICS/resolutionWidth'
Write-Host "  the fixture's resolution is $originalWidth wide"

Write-Host "`nthe test settings file says what it should:"
$testFile = Get-TestSettingsPath
Check 'width is the lowest the game offers' 1280 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/resolutionWidth')
Check 'height matches' 720 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/resolutionHeight')
Check 'windowed (DISPLAY MODE is non-zero)' 1 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/DISPLAY MODE')
Check 'anti-aliasing off' 0 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/ANTI-ALIASING')
Check 'shadows off' 0 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/SHADOWS')
Check 'shader quality low (higher means cheaper)' 1 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/SHADER QUALITY')
Check 'environment FX low (higher hides more)' 1 (Get-GameSetting -Path $testFile -Name 'GRAPHICS/ENVIRONMENT FX')
Check 'tutorial off' $false (Get-GameSetting -Path $testFile -Name 'GRAPHICS/tutorialEnabled')
Check 'music silent' 0 (Get-GameSetting -Path $testFile -Name 'AUDIO/volumeMusic')

Write-Host "`nround trip (registry untouched):"
$ran = $false
Invoke-WithTestSettings -SkipRegistry {
    $script:ran = $true
    Check 'test settings are live inside the block' 1280 (Get-GameSetting -Name 'GRAPHICS/resolutionWidth')
    Check 'and windowed' 1 (Get-GameSetting -Name 'GRAPHICS/DISPLAY MODE')
}

Check 'the script block ran' $true $ran
Check 'the original resolution is back' $originalWidth (Get-GameSetting -Name 'GRAPHICS/resolutionWidth')
Check 'restored byte for byte' `
    ([Convert]::ToBase64String($originalBytes)) `
    ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($scratchFile)))

Write-Host "`nrestores even when the block throws:"
try {
    Invoke-WithTestSettings -SkipRegistry { throw 'the game crashed' }
} catch {
    Write-Host "  (caught: $($_.Exception.Message))"
}
Check 'restored after a throw' `
    ([Convert]::ToBase64String($originalBytes)) `
    ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($scratchFile)))

Write-Host "`nthe installed file is the test file, byte for byte:"
$testBytes = [System.IO.File]::ReadAllBytes($testFile)
Invoke-WithTestSettings -SkipRegistry {
    Check 'installed verbatim' `
        ([Convert]::ToBase64String($testBytes)) `
        ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($scratchFile)))
}

Write-Host "`na missing test file is refused rather than silently skipped:"
$threw = $false
try {
    Invoke-WithTestSettings -SkipRegistry -TestSettingsPath 'no-such-file.json' { }
} catch { $threw = $true }
Check 'missing test settings throws' $true $threw
Check 'and left the settings alone' `
    ([Convert]::ToBase64String($originalBytes)) `
    ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($scratchFile)))

Remove-Item -LiteralPath $scratchDir -Recurse -Force
Remove-Item Env:\DISCO_ELYSIUM_GCT_SETTINGS_FILE

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host 'ALL PASS'
} else {
    Write-Host "FAILURES: $($failures -join ', ')"
    exit 1
}
