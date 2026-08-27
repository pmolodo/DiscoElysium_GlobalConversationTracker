#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

<#
.SYNOPSIS
    Copies a session's BepInEx log out of the game folder and proves the copy
    belongs to the run it is meant to document.

.DESCRIPTION
    BepInEx truncates <game>\BepInEx\LogOutput.log at process start, so a
    session's log only survives until the next launch. "Copy it when the
    session is over" therefore loses the race whenever anything relaunches the
    game in between, and the copy is then a different process's log while
    looking exactly like the right one.

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
        the global state file or a save - must have been written at or after
        the stamp. An artefact OLDER than the stamp proves the log came from a
        later process.
      * If the game is still running, the stamp must fall inside the running
        process's lifetime.
      * With neither of those available there is nothing to check against, and
        an uncheckable artefact is worse than none, so that is a failure too.

    The copy and a <copy>.capture.json manifest (md5, size, the stamp, every
    check and its verdict) are written even when a check fails - a log known to
    be unverified is still worth keeping - but the script then exits non-zero
    unless -Force was given. Nothing is ever written into the game folder.

    The manifest also records what the run was, not just that the log is its:

      * the commit the installed plugin was built from, and whether that tree was
        dirty, read out of the assembly's own informational version.
      * every file in <game>\BepInEx\plugins\GlobalConversationTracker with its
        size and md5, which covers the optional articy id map without naming it.
      * the resync route the log reports and its final average envelope, promoted
        into fields so runs can be compared without re-parsing logs.

    Each is recorded when present and left null when not; a log from a build that
    stamped nothing is a fact worth recording, not a failure.

    A capture and its manifest are paired by name - <log> with <log>.capture.json
    beside it - but renaming a log leaves the manifest behind under the old name.
    The manifest's md5 and byte count describe contents rather than a path, so
    they are the pairing that survives a rename: -Audit pairs by name first and by
    content second, and -Repair re-files a renamed log's manifest beside it.

.PARAMETER GameDir
    The game folder to capture the log out of. Resolved exactly as deploy.ps1
    resolves its target (this parameter, then DISCO_ELYSIUM_DEPLOY_DIR, then the
    Steam copy), so by default the log captured is the one written by the install
    deploy last wrote to. Read-only, like everything else this script does to the
    game folder.

.PARAMETER RunArtifact
    Files the run being captured is claimed to have written - the global state
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

.PARAMETER Rename
    Rename an already-captured log instead of capturing a new one, moving its
    manifest with it: the manifest is renamed to match, its 'copy' field is
    rewritten to the new path, and the old path is kept in 'renamedFrom'. Give the
    log's path here and the new name in -NewName. Renaming by hand leaves the
    manifest behind under the old name, pointing at a path that no longer
    resolves.

.PARAMETER NewName
    The name to give the log named by -Rename. A bare file name renames it where
    it is; a path with a directory moves it there. An existing file is never
    overwritten.

.PARAMETER Force
    Keep the copy and its manifest but report failed cross-checks as warnings
    instead of exiting non-zero. Nothing is hidden: the manifest still records
    verified = false and lists every problem, so a forced capture stays
    identifiable as one that could not be tied to its run.

.PARAMETER Audit
    Re-check the captures already on disk instead of taking a new one: pair every
    <log>.capture.json in -LogDir with the log it describes, re-hash that log and
    compare it against what the manifest recorded, and report every manifest
    whose 'copy' field names a path that is not where its log is now. Exits
    non-zero when a manifest cannot be paired with any log, or when a paired
    log's bytes no longer hash to what was recorded.

    A manifest is paired with its log three ways, in order. By name, which is
    exact: a capture writes the two side by side. By recorded md5 and byte count,
    among logs no manifest of their own has claimed, which survives a rename. By
    shared name tail last - captures are named <label>-<yyyyMMdd-HHmmss>.log, so
    relabelling rewrites the front and leaves the rest. The tail has to start at a
    '-' and carry more than the extension, the longest one wins, and a tie is
    reported rather than guessed at.

    The tail reaches the two cases content cannot: a log edited or truncated since
    capture, whose md5 matches nothing, and several logs sharing one md5 because
    their contents are identical.

.PARAMETER LogDir
    The folder -Audit reads, .build\logs by default. Logs and manifests are
    paired within this folder; a manifest's 'copy' field is reported on, never
    followed.

.PARAMETER Repair
    Fix what the audit can fix: move a manifest that was tracked down by content
    or by name back beside the log it describes, and rewrite its 'copy' field to
    that log's current path. Only the manifest's file name and that one field
    change; the recorded md5, byte count, checks and verdict are left exactly as
    the capture wrote them, so a repaired manifest still says what its run said.

    Implies -Audit, since it repairs that audit's findings, so -Repair on its
    own is the whole job. Pass -Audit alone to look without touching anything.

.EXAMPLE
    .\capture-log.ps1 -Label session-c -RunArtifact "$env:USERPROFILE\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames\global-conversation-state.json"

.EXAMPLE
    .\capture-log.ps1 -Label smoke-test    # while the game is still running

.EXAMPLE
    # Rename a capture and take its manifest with it
    .\capture-log.ps1 -Rename .build\logs\capture-20260819-162547.log `
                      -NewName ApplyRawBytes-Hook-06-skip4tables.log

.EXAMPLE
    .\capture-log.ps1 -Audit               # re-check every capture on disk

.EXAMPLE
    .\capture-log.ps1 -Repair              # and re-file the manifests of renamed logs
#>
[CmdletBinding(DefaultParameterSetName = "Capture")]
param(
    [Parameter(ParameterSetName = "Capture")][string]$GameDir,
    [Parameter(ParameterSetName = "Capture")][string[]]$RunArtifact = @(),
    [Parameter(ParameterSetName = "Capture")][string]$Label = "capture",
    [Parameter(ParameterSetName = "Capture")][string]$Destination,
    [Parameter(ParameterSetName = "Capture")][switch]$Force,
    [Parameter(ParameterSetName = "Rename")][string]$Rename,
    [Parameter(ParameterSetName = "Rename")][string]$NewName,
    [Parameter(ParameterSetName = "Audit")][switch]$Audit,
    [Parameter(ParameterSetName = "Audit")][string]$LogDir,
    [Parameter(ParameterSetName = "Audit")][switch]$Repair
)

$ErrorActionPreference = "Stop"

# A module, not a dot-sourced script, so its names cannot land in this scope and
# overwrite the parameters above. -DisableNameChecking: Assert-NotReferenceCopy
# uses a verb that is not on PowerShell's approved list.
Import-Module (Join-Path $PSScriptRoot "build-support.psm1") -Force -DisableNameChecking

# Where captures live, and what the two halves of one are called.
$DefaultLogDir = Join-Path $BuildDir "logs"
$LogExtension = ".log"
$ManifestSuffix = ".capture.json"
# The manifest's 'copy' field, as the whole of the line that holds it: a repair
# rewrites that one value and leaves every other byte of the file alone, rather
# than reserialising JSON that records evidence. Line-anchored, and a repair
# refuses to touch a manifest where this matches other than exactly once.
$CopyFieldPattern = '(?m)^(\s*"copy"\s*:\s*)"(?:[^"\\]|\\.)*"'

# The Harmony banner's own stamp format, e.g. "### At 2026-08-15 09.34.31".
$HarmonyStampPattern = '(?m)^### At (\d{4}-\d{2}-\d{2} \d{2}\.\d{2}\.\d{2})\s*$'
$HarmonyStampFormat = "yyyy-MM-dd HH.mm.ss"
# How stamps are written into the manifest and the console summary.
$ReportStampFormat = "yyyy-MM-dd HH:mm:ss.fff"
# The resync line names, in parentheses, the route the run took to read the
# save. Builds that do not name a route simply do not match.
$RouteLinePattern = "(?m)^\[\w+\s*:$AssemblyName\] Resynced the global state from the running game \(([^)]+)\)"
# The running average a measured run logs after each hooked call. The last one
# in a log is that run's final figure.
$EnvelopeLinePattern = "(?m)^\[\w+\s*:$AssemblyName\]\s+Average envelope for (\S+)\s*:\s*([\d,]+(?:\.\d+)?) ms \((\d+) calls?\)"
# The banner is written to whole seconds, so a file written in the same second
# as the plugin loaded can look a fraction of a second older than the stamp.
$StampSlack = [TimeSpan]::FromSeconds(1)
# Longest plausible gap between the game process starting and this plugin
# loading. Generous: BepInEx's first run of a game build regenerates the IL2CPP
# interop assemblies before any plugin loads.
$MaxPluginLoadDelay = [TimeSpan]::FromMinutes(15)


function Format-Stamp {
    # One spelling of a timestamp everywhere: console, manifest, messages.
    # PowerShell 5.1's ConvertTo-Json renders DateTime with its own culture rules,
    # so dates go into the manifest already formatted. Untyped, because the checks
    # below legitimately have nothing to format (no banner, no artefact) and a
    # [datetime] parameter cannot hold that.
    param($Value)
    if ($null -eq $Value) { return $null }
    return ([datetime]$Value).ToString($ReportStampFormat)
}


function Get-FileMd5 {
    # One spelling of a file hash everywhere: lower-case hex, as the manifest
    # records it, so a recorded hash and a fresh one compare as plain strings.
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm MD5).Hash.ToLowerInvariant()
}


function Set-ManifestCopyPath {
    # Point a manifest's 'copy' field at where its log actually is, by replacing
    # that one value in the file's text. Everything else the capture recorded -
    # the hash, the checks, the verdict - is left byte for byte as it was.
    param(
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][string]$LogPath
    )

    $text = [System.IO.File]::ReadAllText($ManifestPath)
    $matched = [regex]::Matches($text, $CopyFieldPattern)
    if ($matched.Count -ne 1) {
        throw "$ManifestPath has $($matched.Count) 'copy' fields, expected exactly 1; not touching it."
    }

    # ConvertTo-Json escapes the path into a JSON string literal, quotes and all.
    $m = $matched[0]
    $valueStart = $m.Groups[1].Index + $m.Groups[1].Length
    $newText = $text.Substring(0, $valueStart) + ($LogPath | ConvertTo-Json) + $text.Substring($m.Index + $m.Length)
    [System.IO.File]::WriteAllText($ManifestPath, $newText)
}


function Get-CommonNameTail {
    # The shared end of two file names, cut back to a whole segment, or $null
    # when what they share is no more than the extension.
    param([string]$A, [string]$B)

    $shared = 0
    while ($shared -lt $A.Length -and $shared -lt $B.Length -and
        [char]::ToLowerInvariant($A[$A.Length - 1 - $shared]) -eq
        [char]::ToLowerInvariant($B[$B.Length - 1 - $shared])) {
        $shared++
    }
    if ($shared -eq 0) { return $null }

    # Cut forward to the first separator inside the shared part, so that half of
    # a segment never counts: capture-20260819-162547.log and
    # other-20260820-162547.log share '0-162547.log', and the '0' is a
    # coincidence of two different dates, not a common name.
    $tail = $A.Substring($A.Length - $shared)
    $cut = $tail.IndexOf("-")
    if ($cut -lt 0) { return $null }
    $tail = $tail.Substring($cut)

    # More than the extension has to survive that cut, or every .log in the
    # folder is a match for every other.
    if ($tail.Length -le $LogExtension.Length) { return $null }
    return $tail
}


function Get-NameTailMatch {
    # The one log whose name differs from the one a manifest expects only by a
    # leading segment, or $null if that is not exactly one log. Captures are named
    # <label>-<yyyyMMdd-HHmmss>.log, so relabelling rewrites the front and leaves
    # the tail alone. The longest tail wins, and only if one log alone holds it: a
    # tie is the ambiguity this resolves, not a coin to flip, so it returns
    # nothing and the audit reports it.
    param(
        [Parameter(Mandatory = $true)][string]$ExpectedName,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Candidates
    )

    $best = $null
    $bestLength = 0
    $tied = $false
    foreach ($candidate in $Candidates) {
        $tail = Get-CommonNameTail -A $ExpectedName -B $candidate.Name
        if (-not $tail) { continue }
        if ($tail.Length -gt $bestLength) {
            $best = $candidate
            $bestLength = $tail.Length
            $tied = $false
        }
        elseif ($tail.Length -eq $bestLength) {
            $tied = $true
        }
    }
    if ($tied) { return $null }
    return $best
}


function Get-CapturePairing {
    # Every manifest in a folder, matched to the log it describes: by name, then
    # by recorded md5 and byte count among unclaimed logs, then by shared name
    # tail. See the -Audit help above for why that order. Anything left over is
    # reported rather than guessed at.
    param([Parameter(Mandatory = $true)][string]$LogDir)

    $entries = @(Get-ChildItem -LiteralPath $LogDir -File | Sort-Object Name)
    $logs = @($entries | Where-Object { $_.Extension -eq $LogExtension })
    # Case-insensitively, because the file system these land on is.
    $manifests = @($entries | Where-Object { $_.Name.EndsWith($ManifestSuffix, [System.StringComparison]::OrdinalIgnoreCase) })

    $byName = @{}
    foreach ($log in $logs) { $byName[$log.Name + $ManifestSuffix] = $log }

    # A log with its own manifest beside it is spoken for, and cannot be what
    # some other manifest is looking for.
    $unclaimed = @($logs | Where-Object { -not (Test-Path -LiteralPath (Join-Path $LogDir ($_.Name + $ManifestSuffix))) })

    $results = [System.Collections.Generic.List[object]]::new()
    foreach ($manifest in $manifests) {
        $data = [System.IO.File]::ReadAllText($manifest.FullName) | ConvertFrom-Json
        $md5 = if ($data.md5) { ([string]$data.md5).ToLowerInvariant() } else { $null }

        $log = $byName[$manifest.Name]
        $pairedBy = if ($log) { "name" } else { $null }
        $note = $null

        if (-not $log) {
            $expectedName = $manifest.Name.Substring(0, $manifest.Name.Length - $ManifestSuffix.Length)
            $candidates = @($unclaimed | Where-Object {
                    $_.Length -eq $data.bytes -and (Get-FileMd5 -Path $_.FullName) -eq $md5
                })
            if ($candidates.Count -eq 1) {
                $log = $candidates[0]
                $pairedBy = "md5"
            }
            else {
                # Content could not settle it: nothing carries the recorded md5,
                # or too much does. Narrow to the md5 matches when there were
                # several, otherwise let the name search the whole folder. @()
                # around the whole thing: an empty branch comes out of an
                # if-expression as $null rather than an empty array, which is
                # exactly what a folder of already-claimed logs produces.
                $pool = @(if ($candidates.Count -gt 1) { $candidates } else { $unclaimed })
                $byTail = Get-NameTailMatch -ExpectedName $expectedName -Candidates $pool
                if ($byTail) {
                    $log = $byTail
                    $pairedBy = "name tail"
                }
                elseif ($candidates.Count -eq 0) {
                    $note = "no log in this folder has its recorded md5 $md5, and none is named like $expectedName; the log it describes is not here"
                }
                else {
                    $note = "$($candidates.Count) logs share its recorded md5 $md5 ($($candidates.Name -join ', ')), and no name tail tells them apart"
                }
            }
        }

        $result = [ordered]@{
            manifest = $manifest
            log      = $log
            copy     = $data.copy
            pairedBy = $pairedBy
            note     = $note
            md5Ok    = $null
            bytesOk  = $null
            copyOk   = $null
        }
        if ($log) {
            # Re-hashing on every audit is the point: a manifest that still names
            # its log proves nothing about whether the log still is that log.
            $result.md5Ok = (Get-FileMd5 -Path $log.FullName) -eq $md5
            $result.bytesOk = $log.Length -eq $data.bytes
            $result.copyOk = $data.copy -eq $log.FullName
        }
        $results.Add([pscustomobject]$result)
    }
    return $results
}


function Invoke-CaptureAudit {
    # Re-check every capture in a folder, and optionally put the manifests of
    # renamed logs back where they belong.
    param(
        [Parameter(Mandatory = $true)][string]$LogDir,
        [switch]$Repair
    )

    if (-not (Test-Path -LiteralPath $LogDir)) {
        throw "No captured logs to audit: $LogDir does not exist."
    }
    $LogDir = (Get-Item -LiteralPath $LogDir).FullName

    $pairs = Get-CapturePairing -LogDir $LogDir
    Write-Host "Auditing $($pairs.Count) manifest(s) in $LogDir" -ForegroundColor Cyan

    if ($Repair) {
        # Anything not paired by name is a manifest sitting under the wrong
        # file name, whichever way it was tracked down.
        foreach ($pair in ($pairs | Where-Object { $_.log -and ($_.pairedBy -ne "name" -or -not $_.copyOk) })) {
            Set-ManifestCopyPath -ManifestPath $pair.manifest.FullName -LogPath $pair.log.FullName
            if ($pair.pairedBy -ne "name") {
                $wanted = Join-Path $LogDir ($pair.log.Name + $ManifestSuffix)
                if (Test-Path -LiteralPath $wanted) {
                    throw "Cannot re-file $($pair.manifest.Name) as $($pair.log.Name + $ManifestSuffix): that name is already taken."
                }
                Move-Item -LiteralPath $pair.manifest.FullName -Destination $wanted
                Write-Host "  re-filed $($pair.manifest.Name)" -ForegroundColor Green
                Write-Host "        -> $($pair.log.Name + $ManifestSuffix)"
            }
            else {
                Write-Host "  repointed $($pair.manifest.Name) at $($pair.log.Name)" -ForegroundColor Green
            }
        }
        # Everything below reports on the folder as it now stands.
        $pairs = Get-CapturePairing -LogDir $LogDir
    }

    $unresolved = @($pairs | Where-Object { -not $_.log })
    $corrupt = @($pairs | Where-Object { $_.log -and (-not $_.md5Ok -or -not $_.bytesOk) })
    $renamed = @($pairs | Where-Object { $_.pairedBy -and $_.pairedBy -ne "name" })
    $stale = @($pairs | Where-Object { $_.log -and -not $_.copyOk })

    Write-Host "  paired by name      : $(@($pairs | Where-Object { $_.pairedBy -eq 'name' }).Count)"
    Write-Host "  paired by md5       : $(@($pairs | Where-Object { $_.pairedBy -eq 'md5' }).Count)"
    Write-Host "  paired by name tail : $(@($pairs | Where-Object { $_.pairedBy -eq 'name tail' }).Count)"
    Write-Host "  unresolved          : $($unresolved.Count)"
    Write-Host "  md5 or byte mismatches : $($corrupt.Count)"
    Write-Host "  'copy' fields naming somewhere else : $($stale.Count)"

    $unmanifested = @(Get-ChildItem -LiteralPath $LogDir -File |
            Where-Object { $_.Extension -eq $LogExtension } |
            Where-Object { -not (Test-Path -LiteralPath (Join-Path $LogDir ($_.Name + $ManifestSuffix))) })
    if ($unmanifested.Count -gt 0) {
        Write-Host "  logs with no manifest beside them : $($unmanifested.Count)"
        foreach ($log in $unmanifested) { Write-Host "    $($log.Name)" }
    }

    foreach ($pair in $renamed) {
        Write-Warning "$($pair.manifest.Name) describes $($pair.log.Name) (paired by $($pair.pairedBy)), which was renamed after it was captured. Re-run with -Repair to file it beside that log."
    }
    foreach ($pair in $stale) {
        Write-Warning "$($pair.manifest.Name) records copy = '$($pair.copy)', but its log is at $($pair.log.FullName). Re-run with -Repair to point it there."
    }

    $problems = [System.Collections.Generic.List[string]]::new()
    foreach ($pair in $unresolved) {
        $problems.Add("$($pair.manifest.Name): $($pair.note)")
    }
    foreach ($pair in $corrupt) {
        $problems.Add("$($pair.manifest.Name): $($pair.log.Name) no longer matches what was captured (md5 ok: $($pair.md5Ok), bytes ok: $($pair.bytesOk)).")
    }
    if ($problems.Count -gt 0) {
        throw ("This folder's captures do not all check out:`n" + (($problems | ForEach-Object { "  - $_" }) -join "`n"))
    }

    Write-Host "Every manifest here is paired with a log that still hashes to what was captured." -ForegroundColor Green
}


function Get-LogProvenance {
    # Everything the log says about the process that wrote it: when this plugin
    # loaded in it (the Harmony banner), whether the plugin's own load line is
    # there at all, which global state file it reported writing, and the two
    # figures a comparison between runs is actually made of - the route taken
    # and the final average envelope.
    param([Parameter(Mandatory = $true)][string]$LogPath)

    $LogText = [System.IO.File]::ReadAllText($LogPath)
    $written = (Get-Item -LiteralPath $LogPath).LastWriteTime

    $stamps = [regex]::Matches($LogText, $HarmonyStampPattern)
    $loadStamp = $null
    if ($stamps.Count -gt 0) {
        # The first banner is this plugin patching during Load(); later ones are
        # further patches in the same process.
        $loadStamp = [datetime]::ParseExact(
            $stamps[0].Groups[1].Value, $HarmonyStampFormat,
            [System.Globalization.CultureInfo]::InvariantCulture)
    }

    # Harmony writes a 12-hour clock time with no am/pm designation, so the
    # 24-hour time is ambiguous. If the log's write time is more than 12 hours
    # after the stamp, the stamp must have been pm; add 12 hours to recover it.
    if ($written -gt $loadStamp + [TimeSpan]::FromHours(12))
    {
        $loadStamp = $loadStamp + [TimeSpan]::FromHours(12)
    }

    $version = $null
    $m = [regex]::Match($LogText, "(?m)^\[Message:$AssemblyName\] $AssemblyName v(\S+) loaded\.")
    if ($m.Success) { $version = $m.Groups[1].Value }

    $statePath = $null
    $m = [regex]::Match($LogText, "(?m)^\[Message:$AssemblyName\] Global state file: (.+?)\s*$")
    if ($m.Success) { $statePath = $m.Groups[1].Value }

    # Distinct, in order of appearance: one run takes one route, so more than
    # one here is worth saying out loud rather than silently picking from.
    $routes = [System.Collections.Generic.List[string]]::new()
    foreach ($m in [regex]::Matches($LogText, $RouteLinePattern)) {
        if (-not $routes.Contains($m.Groups[1].Value)) { $routes.Add($m.Groups[1].Value) }
    }

    $operation = $null
    $averageMs = $null
    $callCount = $null
    $averages = [regex]::Matches($LogText, $EnvelopeLinePattern)
    if ($averages.Count -gt 0) {
        # Each line restates the average over every call so far, so the last one
        # is the whole run's figure.
        $final = $averages[$averages.Count - 1]
        $operation = $final.Groups[1].Value
        $averageMs = [double]::Parse(
            $final.Groups[2].Value,
            [System.Globalization.NumberStyles]::Float -bor [System.Globalization.NumberStyles]::AllowThousands,
            [System.Globalization.CultureInfo]::InvariantCulture)
        $callCount = [int]$final.Groups[3].Value
    }

    return [ordered]@{
        LoadStamp         = $loadStamp
        StampCount        = $stamps.Count
        PluginVersion     = $version
        StatePath         = $statePath
        Routes            = $routes
        EnvelopeOperation = $operation
        AverageEnvelopeMs = $averageMs
        EnvelopeCallCount = $callCount
    }
}


function Get-PluginFolderState {
    # Everything installed in the game's plugin folder, with size, write time and
    # md5. Listed wholesale rather than probed for by name, so the optional
    # articy id map - which changes what a run does and how long it takes - is
    # recorded without being special-cased, along with anything else put there.
    # $null when the folder does not exist, which is different from empty.
    #
    # md5 for every file: the folder holds the plugin's own assemblies plus at
    # most a hand-placed data file, and identifying which articy map was in place
    # is worth the few megabytes of hashing.
    param([Parameter(Mandatory = $true)][string]$PluginDir)

    if (-not (Test-Path -LiteralPath $PluginDir)) { return $null }
    return @(Get-ChildItem -LiteralPath $PluginDir -File | Sort-Object Name | ForEach-Object {
            [ordered]@{
                name          = $_.Name
                bytes         = $_.Length
                lastWriteTime = Format-Stamp $_.LastWriteTime
                md5           = Get-FileMd5 -Path $_.FullName
            }
        })
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

# --- 0. The two modes that capture nothing -------------------------------------
# Both run before anything looks at the game: neither renaming a capture nor
# auditing the ones already on disk needs a game folder, a log or a process, and
# handling them here keeps that true.
#
# Renaming is the tidy way to relabel a capture and auditing is the untidy one -
# -Rename moves the manifest with the log, -Repair goes and finds the manifests
# of logs renamed some other way.
if ($Rename -or $NewName) {
    if (-not ($Rename -and $NewName)) {
        throw "-Rename and -NewName go together: -Rename names the captured log, -NewName what to call it."
    }
    Write-Host "Renaming capture:"
    $renamed = Rename-Capture -Path $Rename -NewName $NewName
    Write-Host "Renamed. The manifest travels with the log; anything reading captures should pair them by name."
    return
}

# -Repair implies the audit it fixes the findings of, so it stands on its own.
if ($Audit -or $Repair) {
    if (-not $LogDir) { $LogDir = $DefaultLogDir }
    Invoke-CaptureAudit -LogDir $LogDir -Repair:$Repair
    return
}

# --- 1. Copy the log out, first -----------------------------------------------
# Preserving the bytes beats judging them: every check below can be redone from
# the copy, but a relaunch while we deliberate destroys the original.
$gameDir = Resolve-TargetGameDir -GameDir $GameDir
$logPath = Join-Path $gameDir $BepInExLogRelPath
if (-not (Test-Path -LiteralPath $logPath)) {
    throw "No BepInEx log to capture at $logPath - has the game been run with BepInEx installed?"
}

if (-not $Destination) {
    $Destination = Join-Path $DefaultLogDir "$Label-$(Get-Date -Format 'yyyyMMdd-HHmmss')$LogExtension"
}
$destDir = [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($Destination))
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
Copy-Item -LiteralPath $logPath -Destination $Destination -Force
$Destination = (Get-Item -LiteralPath $Destination).FullName

$copy = Get-Item -LiteralPath $Destination
$source = Get-Item -LiteralPath $logPath
$md5 = Get-FileMd5 -Path $Destination
Write-Host "Captured $logPath" -ForegroundColor Cyan
Write-Host "      -> $Destination"
Write-Host "  md5 $md5  ($($copy.Length) bytes)"

# --- 2. Work out whose log it is ----------------------------------------------
$provenance = Get-LogProvenance -LogPath $Destination
$loadStamp = $provenance.LoadStamp
$game = Get-RunningGameProcess

$problems = [System.Collections.Generic.List[string]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()
$artifacts = [System.Collections.Generic.List[object]]::new()

if (-not $provenance.PluginVersion) {
    $warnings.Add("This log has no '$AssemblyName v<version> loaded.' line, so the plugin never loaded in that process.")
}

if ($provenance.Routes.Count -gt 1) {
    $warnings.Add("This log names more than one resync route ($($provenance.Routes -join ', ')); the manifest records the first.")
}

# --- What was installed, and what it was built from ---------------------------
# The log cannot see the folder its plugin was loaded from, so the folder is read
# here instead. The commit comes off the assembly itself (Get-PluginBuildStamp),
# which is what makes a captured log attributable to a source revision at all.
$pluginDir = Get-PluginInstallDir -GameDir $gameDir
$pluginFiles = Get-PluginFolderState -PluginDir $pluginDir
$pluginDllPath = Join-Path $pluginDir "$AssemblyName.dll"
$pluginBuild = Get-PluginBuildStamp -DllPath $pluginDllPath

if ($null -eq $pluginFiles) {
    $warnings.Add("No plugin folder at $pluginDir, so this manifest cannot record what was installed for this run.")
}
else {
    Write-Host "  installed: $($pluginFiles.Count) file(s) in $pluginDir"
    if (-not $pluginBuild.commit) {
        $warnings.Add("The installed $AssemblyName.dll carries no commit stamp, so this log cannot be tied to a source revision. Redeploy with deploy.ps1, which stamps into the assembly the commit it was built from.")
    }
    else {
        Write-Host "  built from $($pluginBuild.commit)$(if ($pluginBuild.dirty) { " (dirty tree)" })"
        if ($pluginBuild.dirty) {
            $warnings.Add("The installed $AssemblyName.dll was built from commit $($pluginBuild.commit) with uncommitted changes in the tree, so the source behind this log is not any committed state.")
        }
    }

    # The folder is read now; the run happened earlier. A deploy in between makes
    # this listing a later build's than the one that wrote the log.
    if ($loadStamp -and (Test-Path -LiteralPath $pluginDllPath)) {
        $installedWritten = (Get-Item -LiteralPath $pluginDllPath).LastWriteTime
        if ($installedWritten -gt $loadStamp + $StampSlack) {
            $warnings.Add("The installed $AssemblyName.dll was written $(Format-Stamp $installedWritten), after the plugin loaded in this run at $(Format-Stamp $loadStamp): the plugin folder recorded here is a later deploy's, not the one this log came from.")
        }
    }
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
            $warnings.Add("The global state file this log names ($($provenance.StatePath)) was $($checked.note). That run wrote no state, or this is not that run's log.")
        }
        $checked.note = "named by the log itself$(if ($checked.note) { "; $($checked.note)" })"
        $artifacts.Add($checked)
    }
}

# --- 3. Record the verdict next to the copy -----------------------------------
$verified = $problems.Count -eq 0
$manifest = [ordered]@{
    # Identifies the run no matter what the files are called later: the moment
    # of capture plus the head of the copy's own hash. A manifest that has been
    # separated from its log can still be matched back to it by md5 and bytes,
    # and two captures of the same log at different times stay distinguishable.
    runId                = "$(Get-Date -Format 'yyyyMMdd-HHmmss')-$($md5.Substring(0, 8))"
    capturedAt           = Format-Stamp (Get-Date)
    source               = $source.FullName
    sourceLastWriteTime  = Format-Stamp $source.LastWriteTime
    copy                 = $Destination
    md5                  = $md5
    bytes                = $copy.Length
    harmonyLoadStamp     = Format-Stamp $loadStamp
    harmonyStampCount    = $provenance.StampCount
    pluginVersion        = $provenance.PluginVersion
    pluginCommit         = $pluginBuild.commit
    pluginTreeDirty      = $pluginBuild.dirty
    pluginBuildVersion   = $pluginBuild.informationalVersion
    pluginDir            = $pluginDir
    pluginFiles          = $pluginFiles
    route                = if ($provenance.Routes.Count -gt 0) { $provenance.Routes[0] } else { $null }
    envelopeOperation    = $provenance.EnvelopeOperation
    averageEnvelopeMs    = $provenance.AverageEnvelopeMs
    envelopeCallCount    = $provenance.EnvelopeCallCount
    globalStatePath      = $provenance.StatePath
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
$manifestPath = "$Destination$ManifestSuffix"
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
