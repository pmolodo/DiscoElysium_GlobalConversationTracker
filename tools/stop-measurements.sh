#!/usr/bin/env bash
# Stop a measurement run, including the processes that outlive the shell that started it.
#
# Usage:
#   tools/stop-measurements.sh              # what is running, and stop it
#   tools/stop-measurements.sh --list       # say what is running, kill nothing
#
# KILLING THE TERMINAL IS NOT ENOUGH, which is the whole reason this exists. The measurement
# drivers run ONE PROCESS PER GROUP - that is what keeps a crash costing one group rather than
# a whole run - so killing the shell or the job leaves the loop spawning new ones. They then
# hold target/release/examples/menu_matrix.exe open, and the next build fails with LNK1104
# from a run nobody thinks is still going.
#
# This is the stopping incantation as a command rather than a five-line quoted PowerShell
# block, because the block has to be retyped correctly at the moment something is already
# going wrong, and its escaping is exactly what gets mistyped then.
#
# IT KILLS ONLY THIS REPOSITORY'S MEASUREMENTS: the driver scripts by name, and the
# measurement binaries they run. It deliberately does NOT match on `cargo` or `bash` alone.
#
# NOT FOR DOLT OR BEADS. Nothing here touches a database server; see the project rules on
# leaving those to a person.

set -u

LIST_ONLY=0
case "${1:-}" in
    --list) LIST_ONLY=1 ;;
    "") ;;
    *)
        echo "usage: tools/stop-measurements.sh [--list]" >&2
        exit 2
        ;;
esac

# The driver scripts, and the binaries a group runs. Matched on the COMMAND LINE for the
# scripts - they are `python tools/measure-...` or `bash tools/measure-...` and their process
# name is just the interpreter - and on the process NAME for the binaries, which is exact and
# cannot catch an editor that happens to have the path in its title.
read -r -d '' FILTER <<'PS' || true
$scripts = 'measure-menus', 'measure-symbolic'
$binaries = 'menu_matrix.exe', 'menu_matrix'
Get-CimInstance Win32_Process | Where-Object {
    $cmd = $_.CommandLine
    if ($cmd -like '*stop-measurements*') { return $false }
    if ($binaries -contains $_.Name) { return $true }
    if (-not $cmd) { return $false }
    foreach ($s in $scripts) { if ($cmd -like "*$s*") { return $true } }
    return $false
}
PS

if [ "$LIST_ONLY" = 1 ]; then
    powershell -NoProfile -Command "
        $FILTER | Select-Object ProcessId, Name,
            @{ n = 'Started'; e = { \$_.CreationDate } } | Format-Table -AutoSize
        "
    exit 0
fi

powershell -NoProfile -Command "
    \$found = @($FILTER)
    if (\$found.Count -eq 0) {
        Write-Output 'no measurement processes are running'
    } else {
        foreach (\$p in \$found) {
            Write-Output \"stopping \$(\$p.ProcessId) \$(\$p.Name)\"
            Stop-Process -Id \$p.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
    "

# SAID AFTERWARDS RATHER THAN ASSUMED. A run that was killed mid-group has lost that group and
# nothing else: rows are appended as they finish, so everything before it is in the TSV and
# the same command with --resume continues it in the same folder.
echo
echo "rows already finished are in the run's folder; re-running the same command with"
echo "--resume added picks up where this stopped."
