#!/usr/bin/env bash
# Take the unreachable census ONE GROUP PER PROCESS, keeping every row that survives.
#
# The census asks, per group, which of its entries no path can reach under the world's
# conditions - deepest first, up to ten. See the `census` function in
# measurements/performance_matrix.rs for what the columns mean and what `at-least` and
# `undecided` are hiding when they are not zero.
#
# Usage:
#   tools/measure-census.sh [conversation ...|all]
#
# Examples:
#   tools/measure-census.sh 368 631      # just these two
#   tools/measure-census.sh all          # every group in the game, resumably
#
# ONE PROCESS PER GROUP for the reason tools/measure-matrix.sh gives at length: a group can
# take its process down - conversation 28's deepest entries overflow the stack inside a
# recursive diagram operation - and with every group in one process the first crash destroys
# every group after it. A crash here is a RESULT for that group, recorded as CRASHED, and
# costs nothing else.
#
# `all` asks the measurement which groups exist and which have anything in them, exactly as
# the matrix does, rather than reading a list kept here that could omit a group and never
# say so. 901 of the game's 1,422 groups reach nothing from their start and are recorded as
# NO-ROWS from the enumeration - a third of a second for the whole game - instead of by 901
# processes that each build a graph to find the same nothing.
#
# RESUMING. Rows are appended as they finish, and pointing a later run at the same folder
# makes it skip the groups already there:
#
#   CENSUS_OUT=measurements/logs/2026-09-08_census tools/measure-census.sh all
#
# The same command is the start and the resume; there is no separate mode to remember.
# Without CENSUS_OUT each run gets its own folder and resumes nothing.
#
# WHAT COUNTS AS DONE:
#
#   a row       measured, whatever it says. Done.
#   CRASHED     the group took its process down. That IS the answer for that group.
#   NO-ROWS     the enumeration says there is nothing to census here. Also an answer.
#
# READ THE LAST ROW PER conv, which is what the resume does: the file is appended to, so a
# retried group sits after the one it replaces.
#
# STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL, exactly as it does for
# tools/measure-matrix.sh and for the same reason: one process per group means killing the
# terminal or the job leaves this loop spawning new ones, which then hold
# target/release/examples/performance_matrix.exe open and fail the next build with LNK1104,
# from a run nobody thinks is still going.
#
#   tools/stop-measurements.sh --list     # what is running
#   tools/stop-measurements.sh            # stop it
#
# Nothing is lost but the group in flight: rows are appended as they finish, so the same
# command with the same CENSUS_OUT picks up where it stopped.

set -u

# ASKED FOR HELP, NOT FOR A GROUP CALLED `--help`. Without this the flag is read as a
# conversation id, gets its own process, fails, and is recorded as a CRASHED row in a run
# folder created for the occasion - which is a confusing answer to an innocent question, and
# leaves a folder behind to be tidied up.
case "${1:-}" in
    -h|--help)
        sed -n '2,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^# \?//'
        exit 0
        ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

binary="target/release/examples/performance_matrix.exe"
[ -x "$binary" ] || binary="target/release/examples/performance_matrix"

if [ ! -x "$binary" ]; then
    echo "no built measurement at $binary" >&2
    echo "build it first: cargo build --release --example performance_matrix" >&2
    exit 1
fi

stamp="$(date +%Y-%m-%d_%H,%M,%S)"
revision="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    revision="$revision-dirty"
fi

out="${CENSUS_OUT:-measurements/logs/${stamp}_${revision}_census}"
mkdir -p "$out/groups" || exit 1

rows="$out/census.tsv"
enumeration="$out/groups.log"

# THE GROUP LIST COMES FROM THE MEASUREMENT, and so does the answer to whether a group has
# anything to census. Both on one pass, because building each group's graph is what settles
# the second question and doing it twice would be the expensive half done twice.
if [ "${1:-}" = "all" ] || [ $# -eq 0 ]; then
    echo "enumerating the game's groups..."
    GROUPS_ONLY=1 "$binary" >"$out/groups.tsv" 2>"$enumeration"
    groups="$(awk -F'\t' '$4 > 0 { print $1 }' "$out/groups.tsv")"
    empty="$(awk -F'\t' '$4 == 0' "$out/groups.tsv" | wc -l)"
    echo "  $(echo "$groups" | grep -c .) group(s) to census, $empty recorded NO-ROWS"
    awk -F'\t' '$4 == 0 { print $1 "\tNO-ROWS" }' "$out/groups.tsv" >"$out/skipped.tsv"
else
    groups="$(printf '%s\n' "$@")"
    : >"$out/skipped.tsv"
fi

# The header, from the measurement rather than from a copy kept here.
if [ ! -s "$rows" ]; then
    CENSUS=1 CONVERSATION=-1 "$binary" 2>/dev/null | head -1 >"$rows"
fi

done_already() {
    awk -F'\t' -v conv="$1" 'NR > 1 && $1 == conv { found = 1 } END { exit !found }' "$rows"
}

total="$(echo "$groups" | grep -c .)"
index=0
for conversation in $groups; do
    index=$((index + 1))
    if done_already "$conversation"; then
        echo "[$index/$total] $conversation - already done"
        continue
    fi

    log="$out/groups/$conversation.log"
    printf '[%s/%s] %s ... ' "$index" "$total" "$conversation"
    began="$(date +%s)"
    CENSUS=1 NO_HEADER=1 CONVERSATION="$conversation" "$binary" >"$log" 2>&1
    status=$?
    took=$(($(date +%s) - began))

    row="$(grep -E "^$conversation	" "$log" | tail -1)"
    if [ $status -ne 0 ] || [ -z "$row" ]; then
        # A CRASH IS THE ANSWER FOR THIS GROUP, written down so a resume does not retry it
        # for ever and a reader can tell it from a group nobody ran.
        printf '%s\tCRASHED\t\t\t\t%s\t\n' "$conversation" "$((took * 1000))" >>"$rows"
        echo "CRASHED after ${took}s (exit $status) - $log"
        continue
    fi

    printf '%s\n' "$row" >>"$rows"
    echo "${took}s"
done

echo
echo "census: $rows"
