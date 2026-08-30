# SPDX-License-Identifier: MIT
<#
    Exercises game-settings.psm1 against a scratch registry key, never the real
    one. Proves the three things the closure has to get right: an overridden value
    comes back, a value the test ADDED is removed, and a REG_BINARY survives the
    round trip intact.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scratchForReg = 'HKCU\Software\GCT Settings Round Trip Test'
$scratch = 'HKCU:\Software\GCT Settings Round Trip Test'
$env:DISCO_ELYSIUM_GCT_SETTINGS_KEY = $scratchForReg

$failures = @()
function Check($label, $expected, $actual) {
    if ($expected -eq $actual) {
        Write-Host "  PASS  $label"
    } else {
        Write-Host "  FAIL  $label (expected '$expected', got '$actual')"
        $script:failures += $label
    }
}

# A scratch key shaped like the real one: hashed names, a DWORD, a REG_BINARY.
if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
New-Item -Path $scratch -Force | Out-Null
New-ItemProperty -LiteralPath $scratch -Name 'Screenmanager Fullscreen mode_h3630240806' `
    -Value 1 -PropertyType DWord | Out-Null
New-ItemProperty -LiteralPath $scratch -Name 'Screenmanager Resolution Width_h182942802' `
    -Value 3840 -PropertyType DWord | Out-Null
New-ItemProperty -LiteralPath $scratch -Name 'BindingsDefault_h1511096668' `
    -Value ([byte[]](1, 2, 3, 250, 255, 0)) -PropertyType Binary | Out-Null

Import-Module (Join-Path $PSScriptRoot '..\..\game-settings.psm1') -Force

Write-Host "`nkey in use: $(Get-GameSettingsKeyPath -ForRegExe)"
Check 'module targets the scratch key' $scratchForReg (Get-GameSettingsKeyPath -ForRegExe)

Write-Host "`nname resolution (hash suffix found from the bare name):"
Check 'resolves the hashed name' 'Screenmanager Fullscreen mode_h3630240806' `
    (Resolve-GameSettingName -Name 'Screenmanager Fullscreen mode')
Check 'reads through the bare name' 1 (Get-GameSetting -Name 'Screenmanager Fullscreen mode')

Write-Host "`nround trip:"
$ran = $false
Invoke-WithGameSettings -Settings @{
    'Screenmanager Fullscreen mode'  = 3
    'Screenmanager Resolution Width' = 1280
} -ScriptBlock {
    $script:ran = $true
    Check 'override applied inside the block (fullscreen)' 3 `
        (Get-GameSetting -Name 'Screenmanager Fullscreen mode')
    Check 'override applied inside the block (width)' 1280 `
        (Get-GameSetting -Name 'Screenmanager Resolution Width')

    # Something the "test" adds, which a merge-only restore would leave behind.
    New-ItemProperty -LiteralPath $scratch -Name 'LeftBehind_h1' -Value 99 `
        -PropertyType DWord | Out-Null
}

Check 'the script block ran' $true $ran
Check 'fullscreen restored' 1 (Get-GameSetting -Name 'Screenmanager Fullscreen mode')
Check 'width restored' 3840 (Get-GameSetting -Name 'Screenmanager Resolution Width')

$leftover = (Get-Item -LiteralPath $scratch).GetValueNames() | Where-Object { $_ -eq 'LeftBehind_h1' }
Check 'an added value is removed, not merged' $null $leftover

$binary = (Get-ItemProperty -LiteralPath $scratch -Name 'BindingsDefault_h1511096668').'BindingsDefault_h1511096668'
Check 'REG_BINARY survives intact' '1 2 3 250 255 0' ($binary -join ' ')

Write-Host "`nrestores even when the block throws:"
try {
    Invoke-WithGameSettings -Settings @{ 'Screenmanager Fullscreen mode' = 3 } -ScriptBlock {
        throw 'the game crashed'
    }
} catch {
    Write-Host "  (caught: $($_.Exception.Message))"
}
Check 'restored after a throw' 1 (Get-GameSetting -Name 'Screenmanager Fullscreen mode')

Write-Host "`nlaunch arguments:"
Check 'windowed 1280x720' '-screen-width 1280 -screen-height 720 -screen-fullscreen 0' `
    ((Get-GameLaunchArgument -Width 1280 -Height 720 -Windowed) -join ' ')

Remove-Item -LiteralPath $scratch -Recurse -Force
Remove-Item Env:\DISCO_ELYSIUM_GCT_SETTINGS_KEY

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host 'ALL PASS'
} else {
    Write-Host "FAILURES: $($failures -join ', ')"
    exit 1
}
