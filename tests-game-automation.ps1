#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT
<#
    Checks the parts of game-automation.psm1 that can be checked without the game
    running: the screenshot pipeline, the comparison metric, and the key table.

    Deliberately does NOT send input. SendInput goes to whatever has focus, so a
    self-test that pressed keys would type into whatever window happened to be in
    front. That part is exercised by tests-load-save.ps1, against the game.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Import-Module (Join-Path $PSScriptRoot 'game-automation.psm1') -Force

$failures = @()
function Check($label, $expected, $actual) {
    if ($expected -eq $actual) {
        Write-Host "  PASS  $label"
    } else {
        Write-Host "  FAIL  $label (expected '$expected', got '$actual')"
        $script:failures += $label
    }
}
function CheckTrue($label, $condition) { Check $label $true ([bool] $condition) }

function New-SolidBitmap([int] $width, [int] $height, $colour) {
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear($colour)
    } finally {
        $graphics.Dispose()
    }
    return $bitmap
}

Write-Host "`nthe comparison metric:"
$black = New-SolidBitmap 200 100 ([System.Drawing.Color]::Black)
$white = New-SolidBitmap 200 100 ([System.Drawing.Color]::White)
$alsoBlack = New-SolidBitmap 200 100 ([System.Drawing.Color]::Black)
$grey = New-SolidBitmap 200 100 ([System.Drawing.Color]::FromArgb(128, 128, 128))

Check 'identical images differ by 0' 0 ([Math]::Round((Compare-Screenshot -First $black -Second $alsoBlack), 4))
# Not exactly 1: downscaling is bilinear, so the outermost pixels blend with the
# bitmap edge. 0.999 is the right answer, and asserting equality would be
# asserting that the interpolation does not happen.
$opposite = Compare-Screenshot -First $black -Second $white
CheckTrue 'black vs white is as different as it gets' ($opposite -gt 0.99)
Write-Host ("        (black vs white = {0:N4})" -f $opposite)

$greyDiff = Compare-Screenshot -First $black -Second $grey
CheckTrue 'black vs mid-grey lands near the middle' ($greyDiff -gt 0.4 -and $greyDiff -lt 0.6)
Write-Host ("        (black vs grey = {0:N4})" -f $greyDiff)

# A small change in a big image must move the number a little, not a lot - that
# is the property a threshold depends on.
$speck = New-SolidBitmap 200 100 ([System.Drawing.Color]::Black)
$graphics = [System.Drawing.Graphics]::FromImage($speck)
$graphics.FillRectangle([System.Drawing.Brushes]::White, 0, 0, 20, 10)
$graphics.Dispose()
$speckDiff = Compare-Screenshot -First $black -Second $speck
CheckTrue 'a 1% region changing moves the metric only a little' ($speckDiff -gt 0 -and $speckDiff -lt 0.05)
Write-Host ("        (1% of the image changed = {0:N4})" -f $speckDiff)

Write-Host "`nfingerprints:"
$signature = ConvertTo-ScreenshotSignature -Bitmap $black
Check 'a 32x32 fingerprint has 1024 samples' 1024 $signature.Count
Check 'comparing fingerprints matches comparing bitmaps' `
    ([Math]::Round((Compare-Screenshot -First $black -Second $white), 4)) `
    ([Math]::Round((Compare-Screenshot `
        -First (ConvertTo-ScreenshotSignature -Bitmap $black) `
        -Second (ConvertTo-ScreenshotSignature -Bitmap $white)), 4))

$threw = $false
try {
    Compare-Screenshot -First (ConvertTo-ScreenshotSignature -Bitmap $black -Size 16) `
        -Second (ConvertTo-ScreenshotSignature -Bitmap $white -Size 32)
} catch { $threw = $true }
CheckTrue 'mismatched fingerprint sizes are refused' $threw

foreach ($bitmap in @($black, $white, $alsoBlack, $grey, $speck)) { $bitmap.Dispose() }

Write-Host "`nthe key table:"
$keys = Get-GameKeyName
CheckTrue 'the menu keys are all there' `
    (@('Escape', 'Enter', 'Up', 'Down', 'Left', 'Right', 'Space', 'Tab') |
        ForEach-Object { $keys -contains $_ }) -notcontains $false
Check 'letters are there' $true ($keys -contains 'A' -and $keys -contains 'Z')
Check 'digits are there' $true ($keys -contains '0' -and $keys -contains '9')

$threw = $false
try { Send-GameKey -Key 'NoSuchKey' } catch { $threw = $true }
CheckTrue 'an unknown key name is refused rather than ignored' $threw

Write-Host "`ncapture, against this desktop:"
# This shell may have no window of its own when launched from a pipe, so borrow
# any visible one - the capture path is what is under test, not whose window it is.
$host_ = @(Get-Process | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1)
if ($host_.Count -gt 0) {
    $self = $host_[0]
    $window = [pscustomobject]@{ Handle = $self.MainWindowHandle; Process = $self; Title = $self.MainWindowTitle }
    Write-Host "        borrowed window: $($self.ProcessName)"
    $rect = Get-GameWindowRect -Window $window
    Write-Host "        client area: $($rect.Width)x$($rect.Height) at $($rect.X),$($rect.Y)"
    CheckTrue 'the client rect has a positive size' ($rect.Width -gt 0 -and $rect.Height -gt 0)

    $shot = Get-GameScreenshot -Window $window
    try {
        Check 'the screenshot matches the client width' $rect.Width $shot.Width
        Check 'the screenshot matches the client height' $rect.Height $shot.Height
        Check 'comparing a capture with itself gives 0' 0 `
            ([Math]::Round((Compare-Screenshot -First $shot -Second $shot), 4))
    } finally {
        $shot.Dispose()
    }
} else {
    Write-Host '  SKIP  no window on this host (running headless?)'
}

Write-Host "`nfinding a window that is not there:"
Check 'a missing process yields nothing' $null (Find-GameWindow -ProcessName 'no-such-process-xyz')
$threw = $false
try { Wait-GameWindow -ProcessName 'no-such-process-xyz' -TimeoutSeconds 1 } catch { $threw = $true }
CheckTrue 'waiting for one times out loudly' $threw

Write-Host ''
if ($failures.Count -eq 0) {
    Write-Host 'ALL PASS'
} else {
    Write-Host "FAILURES: $($failures -join ', ')"
    exit 1
}
