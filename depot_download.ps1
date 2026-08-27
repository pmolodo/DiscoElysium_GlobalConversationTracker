#!/usr/bin/env -S powershell -NoProfile -ExecutionPolicy Bypass -File
# SPDX-License-Identifier: MIT

<#
.SYNOPSIS
    Downloads a pristine copy of the game - either the current build or the last
    pre-Final-Cut one.

.DESCRIPTION
    A reference copy, straight from Steam's content servers and touched by
    nothing else: no BepInEx, no Vortex, no mod, no plugin folder, none of the
    files the Steam client itself writes into an installed game. That is what
    makes it worth having: a modded install cannot tell you which files the
    game actually ships, and this one can.

    Which version to fetch is the first argument, and there is no default:

        latest          the current public build, whatever it is today
        pre-final-cut   the last build before The Final Cut, pinned to the
                        manifest depot_download_pre-final-cut.bat used to fetch

    It lands in a folder named after the version it is:

        <repo>\.game_reference_copies\steam_<build date>_<edition>_<manifest id>

    All three parts earn their place. The date says which version this is at a
    glance; the edition says which of the two it was asked for, since a bare
    date does not; the manifest id is the exact thing this was downloaded by,
    and is what makes the copy reproducible rather than merely labelled. A
    steam-download.json goes in beside the game files with the build id, every
    timestamp Steam records, and the command that produced it.

    The download is PINNED to a manifest rather than asking for "latest". Asking
    for latest would race: the game can update between the moment this script
    names the folder and the moment the download finishes, and the folder name
    would then be a lie. Resolving the manifest first and downloading that exact
    manifest cannot drift.

    Nothing in the build or deploy scripts calls this, and it does not import
    build-support.psm1. It is a thing you run by hand when a fresh reference
    copy is wanted.

.PARAMETER Username
    The Steam account to download as. Required - PowerShell prompts for it if it
    is left off, including on a -DryRun, because a run that names no account
    cannot do the one thing this script is for. Steam will not serve depot
    content to an account without a licence for the game, and an anonymous login
    is refused outright, so there is no useful default to fall back to.

    With -QrLogin the account is really chosen by whichever account scans the
    code; give the same one here so the run says who it fetched as.

    The first run is interactive - Steam asks for the password and, if the
    account has Steam Guard, a code. -remember-password is passed for you, so
    DepotDownloader caches the credentials and later runs need no typing. If you
    would rather not cache anything, run the depotdownloader command this script
    prints by hand.

    Interactive means interactive: it reads from the terminal, so it cannot be
    run from something that has no keyboard attached to it. -QrLogin is the
    gentler version if you have the Steam mobile app.

.PARAMETER QrLogin
    Log in by scanning a QR code with the Steam mobile app instead of typing a
    password. Still needs a human at the terminal - it draws the code there -
    but nothing secret is typed, and Steam Guard is answered by the same tap
    that scans.

.PARAMETER AppId
    The Steam application. 632470 is Disco Elysium, which is also where The
    Final Cut lives - the same app, a later build.

.PARAMETER DepotId
    The depot holding the Windows game content. An app is made of several
    depots (Windows content, macOS content, soundtrack, redistributables); this
    is the one that is the game.

.PARAMETER Branch
    The Steam branch to take the version from. "public" is the one everybody
    plays.

.PARAMETER Edition
    Which version to fetch. Required, and positional, so `depot_download.ps1
    latest` reads the way it sounds.

        latest         - the branch's current build, resolved fresh every run.
        pre-final-cut  - manifest 3499130543868275315, built 2021-02-11: the
                         last content before The Final Cut landed on 2021-03-30.
                         This is what depot_download_pre-final-cut.bat fetched
                         before it was folded into this script.

    A version worth keeping gets an entry in $Editions rather than a manifest id
    typed on a command line, so that the next person to want it finds a name, a
    date and a reason instead of a number in someone's shell history.

.PARAMETER OutputRoot
    Where the version folder is created. Defaults to .game_reference_copies
    beside this script, which is where the repo keeps every read-only copy of the
    game and of its decompiled output, and which .gitignore keeps out of git. It
    is created if it is not there.

.PARAMETER Validate
    Re-hash files that are already on disk instead of trusting them. Slower, and
    what to use when resuming an interrupted download or checking an existing
    copy has not rotted.

.PARAMETER Force
    Download even though the target folder already exists. Without this, an
    existing folder is left alone and reported - a reference copy that already
    exists is the thing this script is for, and re-fetching 10 GB by accident is
    not.

.PARAMETER DryRun
    Resolve the version, print the folder name and the exact depotdownloader
    command, and stop. Touches nothing and needs no login.

.EXAMPLE
    .\depot_download.ps1 latest -Username someaccount

.EXAMPLE
    # The last build before The Final Cut
    .\depot_download.ps1 pre-final-cut -Username someaccount

.EXAMPLE
    # What would it fetch, and where would it go? Still asks who you are.
    .\depot_download.ps1 latest -Username someaccount -DryRun
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("latest", "pre-final-cut")]
    [string]$Edition,
    [Parameter(Mandatory = $true)]
    [string]$Username,
    [int]$AppId = 632470,
    [int]$DepotId = 632471,
    [string]$Branch = "public",
    [string]$OutputRoot,
    [switch]$QrLogin,
    [switch]$Validate,
    [switch]$Force,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# Where the version metadata comes from. This is a public mirror of what
# `steamcmd +app_info_print` returns, and the reason to use it is that it needs
# no login: the folder can be named before anyone types a password, and -DryRun
# works on a machine with no Steam account at all. If it is ever unreachable,
# the same three values - buildid, timeupdated and the depot's public manifest
# gid - are on SteamDB's page for the app, and can be passed to -Manifest by
# hand.
$AppInfoUrlFormat = "https://api.steamcmd.net/v1/info/{0}"

# The versions worth naming. A pinned entry carries its own date, because the
# branch metadata describes the branch's CURRENT build and says nothing about an
# older manifest.
#
# pre-final-cut is what depot_download_pre-final-cut.bat fetched before it was
# folded into this script, manifest and all. Its date is the manifest's own
# creation time, read with `depotdownloader -manifest-only`, which reports
# "Manifest 3499130543868275315 (02/11/2021 14:16:38)" - 2021-02-11, seven weeks
# before The Final Cut released on 2021-03-30, which is the sanity check that
# says the American date order was read the right way round.
$Editions = @{
    "latest"        = @{
        Manifest = $null
        Date     = $null
    }
    "pre-final-cut" = @{
        Manifest = "3499130543868275315"
        Date     = "2021-02-11"
    }
}

# Installed by: winget install --exact --id SteamRE.DepotDownloader
$DepotDownloaderExe = "depotdownloader"

# Written beside the downloaded game files. Named so it sorts to the top of the
# folder and reads as documentation rather than as something the game shipped.
$MetadataFileName = "steam-download.json"


function Get-AppInfo {
    param([Parameter(Mandatory = $true)][int]$App)

    $url = [string]::Format($AppInfoUrlFormat, $App)
    try {
        $response = Invoke-RestMethod -Uri $url -UseBasicParsing -TimeoutSec 60
    }
    catch {
        throw @"
Could not read the app info for $App from $url
  $($_.Exception.Message)
That service is how this script learns which version is current without logging
in. If it is down, look the values up on https://steamdb.info/app/$App/depots/
and pass the manifest id with -Manifest.
"@
    }
    if ($response.status -ne "success" -or -not $response.data."$App") {
        throw "The app info service had nothing for app ${App}: status '$($response.status)'."
    }
    return $response.data."$App"
}


function ConvertFrom-UnixSeconds {
    param($Seconds)
    if (-not $Seconds) { return $null }
    return [System.DateTimeOffset]::FromUnixTimeSeconds([int64]$Seconds).UtcDateTime
}


try {
    if (-not $OutputRoot) { $OutputRoot = Join-Path $PSScriptRoot ".game_reference_copies" }
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
    $OutputRoot = (Get-Item -LiteralPath $OutputRoot).FullName

    if (-not (Get-Command $DepotDownloaderExe -ErrorAction SilentlyContinue)) {
        throw @"
DepotDownloader is not on PATH.
Install it with:
    winget install --exact --id SteamRE.DepotDownloader
"@
    }

    # --- Which version are we fetching? ---------------------------------------
    $app = Get-AppInfo -App $AppId
    $branchInfo = $app.depots.branches."$Branch"
    if (-not $branchInfo) {
        throw "App $AppId has no '$Branch' branch. Branches: $(($app.depots.branches | Get-Member -MemberType NoteProperty).Name -join ', ')"
    }
    $depotInfo = $app.depots."$DepotId"
    if (-not $depotInfo) {
        $depots = ($app.depots | Get-Member -MemberType NoteProperty).Name | Where-Object { $_ -match "^\d+$" }
        throw "App $AppId has no depot ${DepotId}. Depots: $($depots -join ', ')"
    }

    $currentManifest = $depotInfo.manifests."$Branch".gid
    $pinned = $Editions[$Edition]
    $manifestId = if ($pinned.Manifest) { $pinned.Manifest } else { $currentManifest }
    if (-not $manifestId) {
        throw "Depot $DepotId has no manifest on the '$Branch' branch, so there is no '$Edition' to download."
    }

    # The branch timestamps describe the branch's CURRENT build, so they are only
    # the right answer for the edition that IS the current build. A pinned
    # edition brings its own date with it.
    $isCurrent = $manifestId -eq $currentManifest
    $wentPublic = ConvertFrom-UnixSeconds $branchInfo.timeupdated
    $buildMade = ConvertFrom-UnixSeconds $branchInfo.timebuildupdated

    # Dated by when the build was MADE rather than when it went public, because
    # that is the one definition both editions can answer: it is baked into the
    # manifest itself, where a pinned edition's date comes from. Verified to
    # agree - the manifest reports 06/30/2026 09:25:22 and the branch reports
    # timebuildupdated 09:25:53, half a minute apart. When it went public is in
    # steam-download.json, not lost.
    $versionDate = if ($pinned.Date) { $pinned.Date }
    elseif ($isCurrent -and $buildMade) { $buildMade.ToString("yyyy-MM-dd") }
    else { "unknown-date" }

    $targetName = "steam_${versionDate}_${Edition}_$manifestId"
    $targetDir = Join-Path $OutputRoot $targetName

    Write-Host "App        : $AppId ($($app.common.name))"
    Write-Host "Depot      : $DepotId"
    Write-Host "Edition    : $Edition"
    Write-Host "Branch     : $Branch, build $($branchInfo.buildid)"
    if ($isCurrent) {
        Write-Host "Version    : built $($buildMade.ToString('yyyy-MM-dd HH:mm:ss')) UTC, went public $($wentPublic.ToString('yyyy-MM-dd HH:mm:ss')) UTC"
    }
    else {
        Write-Host "Version    : built $versionDate, pinned - the branch metadata above describes today's build, not this one"
    }
    Write-Host "Manifest   : $manifestId$(if ($isCurrent) { ' (current)' } else { " (current is $currentManifest)" })"
    # Only for the current manifest: the branch metadata's sizes describe the
    # branch's build, and printing them next to a pinned older manifest would be
    # quoting one version's size for another's download.
    if ($isCurrent -and $depotInfo.manifests."$Branch".size) {
        $installGb = [math]::Round([double]$depotInfo.manifests."$Branch".size / 1GB, 1)
        $downloadGb = [math]::Round([double]$depotInfo.manifests."$Branch".download / 1GB, 1)
        Write-Host "Size       : $downloadGb GB to download, $installGb GB on disk"
    }
    Write-Host "Destination: $targetDir"

    $arguments = @(
        "-app", $AppId,
        "-depot", $DepotId,
        "-manifest", $manifestId,
        "-dir", $targetDir
    )
    $arguments += @("-username", $Username, "-remember-password")
    if ($QrLogin) { $arguments += "-qr" }
    if ($Validate) { $arguments += "-validate" }

    Write-Host ""
    Write-Host "Command    : $DepotDownloaderExe $($arguments -join ' ')"

    if ($DryRun) {
        Write-Host ""
        Write-Host "-DryRun given; nothing downloaded."
        exit 0
    }

    if ((Test-Path -LiteralPath $targetDir) -and -not $Force -and -not $Validate) {
        Write-Host ""
        Write-Host "That folder already exists, so this version has been fetched before."
        Write-Host "Nothing was downloaded. -Validate re-checks it against the manifest; -Force downloads over it."
        exit 0
    }

    # Said before the first prompt rather than after a confusing failure: this
    # step reads from the terminal, and a shell with nothing attached to its
    # stdin gets "Access token was rejected (AccessDenied)" instead of a prompt.
    Write-Host "Steam login is interactive on the first run for an account; answer the prompt(s) below."

    # --- Fetch -----------------------------------------------------------------
    Write-Host ""
    & $DepotDownloaderExe @arguments
    if ($LASTEXITCODE -ne 0) {
        $state = if (Test-Path -LiteralPath $targetDir) {
            "The partial copy at $targetDir was left in place; re-run with -Validate to resume it, or delete it."
        }
        else {
            "Nothing was written, so there is nothing to clean up."
        }
        throw @"
depotdownloader exited with code $LASTEXITCODE.
$state
If it said "Access token was rejected (AccessDenied)" without ever asking for a
password, it had no terminal to ask on: run this from an interactive shell.
"@
    }

    # --- Say what this is, beside the files it describes ------------------------
    $metadata = [ordered]@{
        appId              = $AppId
        appName            = $app.common.name
        edition            = $Edition
        versionDate        = $versionDate
        depotId            = $DepotId
        branch             = $Branch
        buildId            = $branchInfo.buildid
        manifestId         = $manifestId
        isBranchCurrent    = $isCurrent
        buildMadeUtc       = if ($buildMade) { $buildMade.ToString("yyyy-MM-dd HH:mm:ss") } else { $null }
        wentPublicUtc      = if ($wentPublic) { $wentPublic.ToString("yyyy-MM-dd HH:mm:ss") } else { $null }
        downloadedAtUtc    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd HH:mm:ss")
        downloadedBy       = "$DepotDownloaderExe $(& $DepotDownloaderExe --version 2>&1 | Select-Object -First 1)"
        command            = "$DepotDownloaderExe $($arguments -join ' ')"
        note               = "Pristine Steam content. No BepInEx, no mods, nothing the Steam client writes into an installed game. Reproduce with the command above."
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $targetDir $MetadataFileName),
        ($metadata | ConvertTo-Json -Depth 5))

    $files = @(Get-ChildItem -LiteralPath $targetDir -Recurse -File -Force)
    $sizeGb = [math]::Round((($files | Measure-Object -Property Length -Sum).Sum) / 1GB, 2)

    Write-Host ""
    Write-Host "Downloaded $($files.Count) file(s), $sizeGb GB, to:" -ForegroundColor Green
    Write-Host "  $targetDir"
    Write-Host "What it is, and how to get it again, is recorded in $MetadataFileName beside them."
}
catch {
    Write-Host ""
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}
