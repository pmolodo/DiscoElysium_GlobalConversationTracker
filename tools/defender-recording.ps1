# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
    Records Microsoft Defender's performance, elevated, for a measurement that is not.

.DESCRIPTION
    What `New-MpPerformanceRecording` does, as a process of its own that a measurement starts
    with a UAC prompt and talks to through files in one folder. The cmdlet waits on the console
    for ENTER, which a program cannot press, so this runs the same WPR commands with the same
    profile - see the module's MSFT_MpPerformanceRecording.psm1 - and Get-MpPerformanceReport
    reads what they write exactly as it reads the cmdlet's.

    The files, all in -Folder:
      defender.started   written once the recording is running; the measurement waits for it
      defender.stop      written by the measurement when it is done
      defender.etl       the recording, kept for any report Get-MpPerformanceReport can give
      defender-report.txt  the paths its scans cost most in
      defender.done      written once the recording and the report are on disk
      defender.failed    written instead, with what went wrong

    STOPPED EITHER WAY. A measurement that dies never writes defender.stop, so the recording is
    also stopped and saved once the process named by -Watch is gone - a WPR session is the
    machine's, and one left running would refuse the next.

.PARAMETER Folder
    Where the files go.

.PARAMETER Watch
    The measurement's process id.
#>
param(
    [Parameter(Mandatory = $true)][string]$Folder,
    [Parameter(Mandatory = $true)][int]$Watch
)

$ErrorActionPreference = 'Stop'
$instance = 'MSFT_MpPerformanceRecording'
$started = Join-Path $Folder 'defender.started'
$stop = Join-Path $Folder 'defender.stop'
$etl = Join-Path $Folder 'defender.etl'
$report = Join-Path $Folder 'defender-report.txt'
$done = Join-Path $Folder 'defender.done'
$failed = Join-Path $Folder 'defender.failed'

function Invoke-Wpr {
    # A PROGRAM'S STDERR IS AN ERROR RECORD in Windows PowerShell, and under 'Stop' the first
    # line of it ends the script - even the "nothing to cancel" a harmless cancel prints. So WPR
    # runs with what it says merely collected, and is judged by its exit code.
    $ErrorActionPreference = 'Continue'
    $said = & wpr @args 2>&1 | Out-String
    [pscustomobject]@{ Code = $LASTEXITCODE; Said = $said }
}

try {
    $module = Get-Module -ListAvailable ConfigDefenderPerformance | Select-Object -First 1
    if (-not $module) {
        throw 'the ConfigDefenderPerformance module, which holds the recording profile, is not installed'
    }
    $profilePath = Join-Path $module.ModuleBase 'MSFT_MpPerformanceRecording.wprp'

    # A RECORDING LEFT RUNNING is cancelled first, as the cmdlet does; with none, this fails
    # and says nothing that matters.
    Invoke-Wpr -cancel -instancename $instance | Out-Null
    $ran = Invoke-Wpr -start "$profilePath!Scans.Light" -filemode -instancename $instance
    if ($ran.Code -ne 0) {
        throw ("wpr -start failed ({0:x}): {1}" -f $ran.Code, $ran.Said)
    }
    Set-Content -LiteralPath $started -Value (Get-Date -Format o)

    while (-not (Test-Path -LiteralPath $stop)) {
        if (-not (Get-Process -Id $Watch -ErrorAction SilentlyContinue)) {
            break
        }
        Start-Sleep -Milliseconds 500
    }

    $ran = Invoke-Wpr -stop $etl -instancename $instance
    if ($ran.Code -ne 0) {
        throw ("wpr -stop failed ({0:x}): {1}" -f $ran.Code, $ran.Said)
    }
    # WITHOUT A BYTE-ORDER MARK, which Windows PowerShell's own utf8 writes put first.
    $text = Get-MpPerformanceReport -Path $etl -TopPaths 10 | Out-String -Width 250
    [System.IO.File]::WriteAllText($report, $text, (New-Object System.Text.UTF8Encoding $false))
    Set-Content -LiteralPath $done -Value (Get-Date -Format o)
}
catch {
    $_ | Out-String | Set-Content -LiteralPath $failed
    Invoke-Wpr -cancel -instancename $instance | Out-Null
    exit 1
}
