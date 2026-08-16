#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File

<#
.SYNOPSIS
    Copies a session's BepInEx log out of the game folder and proves the copy
    belongs to the run it is meant to document.

.DESCRIPTION
    BepInEx truncates <game>\BepInEx\LogOutput.log at process start, so a
    session's log only survives until the next launch. "Copy it when the
    session is over" therefore loses the race whenever anything relaunches the
    game in between, and the copy is a different process's log while looking
    exactly like the right one - see de-g1z, where a preserved session log
    turned out to come from a launch 44 seconds AFTER that session had already
    finished writing its state file, and a verification spot-check then read a
    conclusion out of the wrong file.

    So this script does two things, in this order:

      1. Copies the log out of the game folder immediately, before anything can
         relaunch the game (to .build\logs\ unless -Destination says otherwise).
      2. Cross-checks that copy against the run it claims to document, and says
         loudly - by failing - when the evidence does not line up.

    The cross-check hangs off the Harmony banner written while the plugin is
    patching inside Load():

        ### At 2026-08-15 09.34.31

    That stamp is the moment this plugin loaded in the process that wrote the
    log, which makes it a fingerprint of the run:

      * Every -RunArtifact - a file that run is claimed to have written, e.g.
        the unified state file or a save - must have been written at or after
        the stamp. An artefact OLDER than the stamp proves the log came from a
        later process, which is exactly how de-g1z was found.
      * If the game is still running, the stamp must fall inside the running
        process's lifetime.
      * With neither of those available there is nothing to check against, and
        an uncheckable artefact is worse than none, so that is a failure too.

    The copy and a <copy>.capture.json manifest (md5, size, the stamp, every
    check and its verdict) are written even when a check fails - a log known to
    be unverified is still worth keeping - but the script then exits non-zero
    unless -Force was given. Nothing is ever written into the game folder.

.PARAMETER GameDir
    The game folder to capture the log out of. Resolved exactly as deploy.ps1
    resolves its target (this parameter, then DISCO_ELYSIUM_DEPLOY_DIR, then the
    Steam copy), so by default the log captured is the one written by the install
    deploy last wrote to. Read-only, like everything else this script does to the
    game folder.

.PARAMETER RunArtifact
    Files the run being captured is claimed to have written - the unified state
    file, a save - each of which is evidence of when that run was still going.
    Every one of them must have been written at or after the log's Harmony
    plugin-load stamp; one that is older proves the log belongs to a later
    launch, and the capture fails. Giving none is only safe while the game is
    still running: with no artefact and no live process there is nothing to check
    against, which is itself a failure.

.PARAMETER Label
    Names the capture in the generated file name,
    .build\logs\<Label>-<yyyyMMdd-HHmmss>.log. Defaults to "capture", and has no
    effect when -Destination names the file outright.

.PARAMETER Destination
    Write the copy here instead of to the generated name under .build\logs.
    Missing directories are created, an existing file is overwritten, and the
    manifest lands beside it as <Destination>.capture.json.

.PARAMETER Force
    Keep the copy and its manifest but report failed cross-checks as warnings
    instead of exiting non-zero. Nothing is hidden: the manifest still records
    verified = false and lists every problem, so a forced capture stays
    identifiable as one that could not be tied to its run.

.EXAMPLE
    .\capture-log.ps1 -Label session-c -RunArtifact "$env:USERPROFILE\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames\unified-conversation-state.json"

.EXAMPLE
    .\capture-log.ps1 -Label smoke-test    # while the game is still running
#>
[CmdletBinding()]
param(
    [string]$GameDir,
    [string[]]$RunArtifact = @(),
    [string]$Label = "capture",
    [string]$Destination,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

# Shared project config: $BepInExLogRelPath, $BuildDir, Resolve-TargetGameDir,
# Invoke-ScriptMain, ... A module, not a dot-sourced script, so that its own
# names cannot land in this script's scope and overwrite the parameters above -
# see the header of build-support.psm1 (de-3pw).
# -DisableNameChecking: Assert-NotReferenceCopy uses a verb PowerShell does not
# have on its approved list, and the name says what it does better than any
# approved verb would.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

# The Harmony banner's own stamp format, e.g. "### At 2026-08-15 09.34.31".
$HarmonyStampPattern = '(?m)^### At (\d{4}-\d{2}-\d{2} \d{2}\.\d{2}\.\d{2})\s*$'
$HarmonyStampFormat = "yyyy-MM-dd HH.mm.ss"
# How stamps are written into the manifest and the console summary.
$ReportStampFormat = "yyyy-MM-dd HH:mm:ss.fff"
# The banner is written to whole seconds, so a file written in the same second
# as the plugin loaded can look a fraction of a second older than the stamp.
$StampSlack = [TimeSpan]::FromSeconds(1)
# Longest plausible gap between the game process starting and this plugin
# loading. Generous: BepInEx's first run of a game build regenerates the IL2CPP
# interop assemblies before any plugin loads.
$MaxPluginLoadDelay = [TimeSpan]::FromMinutes(15)


function Format-Stamp {
    # One spelling of a timestamp everywhere: console, manifest, messages.
    # PowerShell 5.1's ConvertTo-Json renders DateTime with its own culture
    # rules, so dates go into the manifest already formatted.
    # Untyped, because the checks below legitimately have nothing to format
    # (no banner in the log, no artefact on disk) and a [datetime] parameter
    # cannot hold that.
    param($Value)
    if ($null -eq $Value) { return $null }
    return ([datetime]$Value).ToString($ReportStampFormat)
}


function Get-LogProvenance {
    # Everything the log says about the process that wrote it: when this plugin
    # loaded in it (the Harmony banner), whether the plugin's own load line is
    # there at all, and which unified state file it reported writing.
    param([Parameter(Mandatory = $true)][string]$LogText)

    $stamps = [regex]::Matches($LogText, $HarmonyStampPattern)
    $loadStamp = $null
    if ($stamps.Count -gt 0) {
        # The first banner is this plugin patching during Load(); later ones are
        # further patches in the same process.
        $loadStamp = [datetime]::ParseExact(
            $stamps[0].Groups[1].Value, $HarmonyStampFormat,
            [System.Globalization.CultureInfo]::InvariantCulture)
    }

    $version = $null
    $m = [regex]::Match($LogText, "(?m)^\[Message:$AssemblyName\] $AssemblyName v(\S+) loaded\.")
    if ($m.Success) { $version = $m.Groups[1].Value }

    $statePath = $null
    $m = [regex]::Match($LogText, "(?m)^\[Message:$AssemblyName\] Unified state file: (.+?)\s*$")
    if ($m.Success) { $statePath = $m.Groups[1].Value }

    return [ordered]@{
        LoadStamp     = $loadStamp
        StampCount    = $stamps.Count
        PluginVersion = $version
        StatePath     = $statePath
    }
}


function Get-RunningGameProcess {
    # The running game process, if any, with its start time - the strongest
    # evidence available, because a log being written by a live process cannot
    # also be a later launch's. $null when the game is not running or when its
    # start time cannot be read.
    $name = [System.IO.Path]::GetFileNameWithoutExtension($GameExeName)
    $procs = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    foreach ($proc in ($procs | Sort-Object -Property Id)) {
        try {
            return [ordered]@{ Id = $proc.Id; StartTime = $proc.StartTime }
        }
        catch {
            # Start time needs rights we may not have; try the next one.
        }
    }
    return $null
}


function Test-ArtifactWrittenByRun {
    # One run artefact against the plugin-load stamp. An artefact written
    # BEFORE the plugin loaded cannot have been written by the process that
    # wrote this log, so the log is a different launch's.
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][datetime]$LoadStamp
    )

    # lastWriteTime stays a DateTime here so callers can compare it; the
    # manifest formats it on the way out.
    $result = [ordered]@{ path = $Path; lastWriteTime = $null; ok = $false; note = $null }
    if (-not (Test-Path -LiteralPath $Path)) {
        $result.note = "not found"
        return $result
    }
    $written = (Get-Item -LiteralPath $Path).LastWriteTime
    $result.lastWriteTime = $written
    if ($written -lt $LoadStamp - $StampSlack) {
        $result.note = "written $(Format-Stamp $written), which is BEFORE the plugin loaded at $(Format-Stamp $LoadStamp)"
        return $result
    }
    $result.ok = $true
    return $result
}


Invoke-ScriptMain {

# --- 1. Copy the log out, first -----------------------------------------------
# Preserving the bytes beats judging them: every check below can be redone from
# the copy, but a relaunch while we deliberate destroys the original.
$gameDir = Resolve-TargetGameDir -GameDir $GameDir
$logPath = Join-Path $gameDir $BepInExLogRelPath
if (-not (Test-Path -LiteralPath $logPath)) {
    throw "No BepInEx log to capture at $logPath - has the game been run with BepInEx installed?"
}

if (-not $Destination) {
    $Destination = Join-Path $BuildDir "logs\$Label-$(Get-Date -Format 'yyyyMMdd-HHmmss').log"
}
$destDir = [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($Destination))
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
Copy-Item -LiteralPath $logPath -Destination $Destination -Force
$Destination = (Get-Item -LiteralPath $Destination).FullName

$copy = Get-Item -LiteralPath $Destination
$source = Get-Item -LiteralPath $logPath
$md5 = (Get-FileHash -LiteralPath $Destination -Algorithm MD5).Hash.ToLowerInvariant()
Write-Host "Captured $logPath" -ForegroundColor Cyan
Write-Host "      -> $Destination"
Write-Host "  md5 $md5  ($($copy.Length) bytes)"

# --- 2. Work out whose log it is ----------------------------------------------
$provenance = Get-LogProvenance -LogText ([System.IO.File]::ReadAllText($Destination))
$loadStamp = $provenance.LoadStamp
$game = Get-RunningGameProcess

$problems = [System.Collections.Generic.List[string]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()
$artifacts = [System.Collections.Generic.List[object]]::new()

if (-not $provenance.PluginVersion) {
    $warnings.Add("This log has no '$AssemblyName v<version> loaded.' line, so the plugin never loaded in that process.")
}

if (-not $loadStamp) {
    $problems.Add(@"
No Harmony '### At <date>' banner in this log, so there is nothing to identify
the process that wrote it. That banner appears when the plugin patches during
Load(); a log without one is either from a launch where the plugin never loaded
or from a BepInEx configured to log less than it does by default.
"@.Trim())
}
else {
    Write-Host "  plugin loaded at $(Format-Stamp $loadStamp)$(if ($provenance.PluginVersion) { " (v$($provenance.PluginVersion))" })"

    foreach ($path in $RunArtifact) {
        $checked = Test-ArtifactWrittenByRun -Path $path -LoadStamp $loadStamp
        $artifacts.Add($checked)
        if (-not $checked.ok) {
            $problems.Add("Run artefact '$path' was $($checked.note). This log is NOT that run's log.")
        }
        elseif ($checked.lastWriteTime -gt $copy.LastWriteTime) {
            $warnings.Add("Run artefact '$path' is newer than the captured log; expected while the game is still running, suspect otherwise.")
        }
    }

    if ($game) {
        $started = $game.StartTime
        if ($loadStamp -lt $started - $StampSlack) {
            $problems.Add("The running game (pid $($game.Id)) started at $(Format-Stamp $started), after this log says the plugin loaded ($(Format-Stamp $loadStamp)): the log on disk is already a different process's.")
        }
        elseif ($loadStamp -gt $started + $MaxPluginLoadDelay) {
            $warnings.Add("The plugin load stamp is more than $($MaxPluginLoadDelay.TotalMinutes) minutes after the running game (pid $($game.Id)) started; check this is the same process.")
        }
        else {
            Write-Host "  matches the running game (pid $($game.Id), started $(Format-Stamp $started))"
        }
    }
    elseif ($RunArtifact.Count -eq 0) {
        $problems.Add(@"
Nothing to cross-check this log against: the game is not running and no
-RunArtifact was given. The log on disk may already belong to a later launch,
and there would be no way to tell. Pass -RunArtifact <a file the run wrote>, or
-Force to keep this copy marked unverified.
"@.Trim())
    }

    # The log names the state file it wrote, so it can check itself - softly,
    # because a run that changed nothing legitimately leaves an older file.
    if ($provenance.StatePath -and -not ($RunArtifact -contains $provenance.StatePath)) {
        $checked = Test-ArtifactWrittenByRun -Path $provenance.StatePath -LoadStamp $loadStamp
        if (-not $checked.ok) {
            $warnings.Add("The unified state file this log names ($($provenance.StatePath)) was $($checked.note). That run wrote no state, or this is not that run's log.")
        }
        $checked.note = "named by the log itself$(if ($checked.note) { "; $($checked.note)" })"
        $artifacts.Add($checked)
    }
}

# --- 3. Record the verdict next to the copy -----------------------------------
$verified = $problems.Count -eq 0
$manifest = [ordered]@{
    capturedAt           = Format-Stamp (Get-Date)
    source               = $source.FullName
    sourceLastWriteTime  = Format-Stamp $source.LastWriteTime
    copy                 = $Destination
    md5                  = $md5
    bytes                = $copy.Length
    harmonyLoadStamp     = Format-Stamp $loadStamp
    harmonyStampCount    = $provenance.StampCount
    pluginVersion        = $provenance.PluginVersion
    unifiedStatePath     = $provenance.StatePath
    gameProcessId        = if ($game) { $game.Id } else { $null }
    gameProcessStartTime = if ($game) { Format-Stamp $game.StartTime } else { $null }
    runArtifacts         = @($artifacts | ForEach-Object {
            [ordered]@{
                path          = $_.path
                lastWriteTime = Format-Stamp $_.lastWriteTime
                ok            = $_.ok
                note          = $_.note
            }
        })
    warnings             = @($warnings)
    problems             = @($problems)
    verified             = $verified
}
$manifestPath = "$Destination.capture.json"
# WriteAllText rather than Set-Content: no BOM, so anything can read the JSON.
[System.IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 5))
Write-Host "  manifest $manifestPath"

foreach ($warning in $warnings) { Write-Warning $warning }

# The log is truncated at every launch unless BepInEx is told otherwise, which
# is what makes this whole race possible. Report it; the config is the user's
# (see the safety rules in DEVELOPING.md), so nothing here edits it.
$configPath = Join-Path $gameDir $BepInExConfigRelPath
if (Test-Path -LiteralPath $configPath) {
    $cfg = [System.IO.File]::ReadAllText($configPath)
    if (-not [regex]::IsMatch($cfg, '(?ms)^\[Logging\.Disk\][^\[]*?^AppendLog\s*=\s*true')) {
        Write-Host "Tip: [Logging.Disk] AppendLog = true in $configPath keeps every session in one log instead of overwriting it at each launch."
    }
}

if (-not $verified) {
    $bullets = $problems | ForEach-Object { "  - " + ($_ -replace "`n", "`n    ") }
    $summary = "This capture could not be tied to the run it is supposed to document:`n" + ($bullets -join "`n")
    if ($Force) {
        Write-Warning "$summary`n-Force was given, so the copy is kept - the manifest records verified = false."
        return
    }
    throw "$summary`nThe copy and its manifest were kept (verified = false). Re-run with -Force to accept it as unverified."
}

Write-Host "Verified: this log belongs to the run it was captured for." -ForegroundColor Green

}
