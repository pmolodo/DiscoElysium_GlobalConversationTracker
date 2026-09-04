#!/usr/bin/env bash
# Run the performance matrix ONE ROW PER PROCESS, keeping every row that survives.
#
# A row can take the whole process down. Measured: conversation 28 with its five deepest
# entries unseen overflows the stack inside a recursive diagram operation, and the run that
# found that reported two rows out of eleven and lost the other nine. Others die by the
# diagram manager running out of nodes, or by a single step running minutes past its
# budget.
#
# One process per row means a crash is a RESULT for that row - recorded as such - and costs
# nothing else. That is the same bargain tools/measure-symbolic.sh makes, for the same
# reason; this one is finer grained because the matrix has far more rows per conversation.
#
# Usage:
#   tools/measure-matrix.sh [conversation ...]
#
# Examples:
#   tools/measure-matrix.sh                 # every conversation, every profile
#   tools/measure-matrix.sh 368 631         # just these two
#
# STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL. Every row is a fresh cargo and a
# fresh test binary, so killing the terminal or the job leaves the script looping and
# starting new ones - which then hold the test binary open and fail the next build with
# LNK1104, from a run nobody thinks is still going. Kill by command line:
#
#   powershell -NoProfile -Command "Get-CimInstance Win32_Process |
#     Where-Object { \$_.CommandLine -like '*measure-matrix*' -or
#                    \$_.CommandLine -like '*performance_matrix*' } |
#     ForEach-Object { Stop-Process -Id \$_.ProcessId -Force }"
#
# Writes one folder per run under measurements/logs, holding a log per row AND the TSV
# each conversation's rows were collected into.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/measurements"

# ONE FOLDER PER RUN, named for the day and the commit it measured - the whole matrix is
# one measurement, and its sixty-six row logs only mean anything as a set. Flat files
# named for the row were overwritten by the next run, which left every recorded TSV with
# no logs behind it except the newest one's.
#
# THE TSVs GO IN HERE TOO, rather than into measurements/ where they were committed. A row
# is a wall-clock time on one machine and it moves whenever anything about the search does,
# so a committed one is a baseline that is wrong more often than it is right - and one that
# is wrong silently, because nothing re-runs it. Keeping the summary beside the logs it was
# drawn from is what makes a run readable later; comparing two runs means comparing two
# folders, which is the honest shape of the comparison anyway.
LOGS="$(RUN_LOG_DIR="$OUT/logs" "$ROOT/tools/run-logged.sh" --folder-only measure matrix)"
mkdir -p "$LOGS"

CONVERSATIONS=("$@")
if [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(362 368 631 14 28 1030)
fi

# Counted so the run can say at the end that part of it is not a measurement. A line
# scrolled past an hour ago is not a warning.
not_measured=0

PROFILES=(
    all-seen
    deepest-1
    deepest-5
    deepest-10
    95pc-seen
    90pc-seen
    75pc-seen
    50pc-seen
    25pc-seen
    10pc-seen
    5pc-seen
)

HEADER=$'conv\tentries\tprofile\tunseen\tfwd_verdict\tfwd_ms\tfwd_states\tbwd_verdict\tbwd_ms\tbwd_nodes\tbwd_setsum'

# Built once, up front. Letting each row build would put a compile inside the timing of
# whichever row happened to run first.
echo "building..."
cargo build --release --tests --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1

for conversation in "${CONVERSATIONS[@]}"; do
    tsv="$LOGS/performance-matrix-$conversation.tsv"
    echo "$HEADER" > "$tsv"
    echo "=== $conversation -> $tsv"

    for profile in "${PROFILES[@]}"; do
        log="$LOGS/matrix-$conversation-$profile.log"
        printf '  %-12s' "$profile"

        CONVERSATION="$conversation" PROFILE="$profile" NO_HEADER=1 \
            cargo test --release --test performance_matrix \
            --manifest-path "$ROOT/Cargo.toml" \
            -- --ignored --nocapture > "$log" 2>&1

        row="$(grep -E "^$conversation\b" "$log" | head -1)"
        if [ -n "$row" ]; then
            echo "$row" >> "$tsv"

            # THREE OUTCOMES, NOT TWO, and the third is not a result. The test prints a
            # NOT-MEASURED row when the machine could not supply the budget; that says
            # nothing about the search and the run wants repeating with the memory free.
            # Flattening it in with the real rows is how a gap gets read as a finding.
            case "$row" in
                *NOT-MEASURED*)
                    echo "NOT MEASURED - no memory for the budget; rerun this row"
                    not_measured=$((not_measured + 1))
                    ;;
                *) echo "ok" ;;
            esac
        else
            # A CRASH IS A RESULT. The row says so and names its log, rather than being
            # silently absent - an empty line in a measurement reads as "not run yet",
            # which is a different thing from "this is what happens". Distinct from
            # NOT-MEASURED above: this row died, that one never ran.
            echo -e "$conversation\t?\t$profile\t?\tCRASHED\t?\t?\tCRASHED\t?\t?\t?" >> "$tsv"
            echo "CRASHED (see $log)"
        fi
    done
done

echo
echo "wrote:"
ls -1 "$LOGS"/performance-matrix-*.tsv
echo "logs for this run: $LOGS"

if [ "$not_measured" -gt 0 ]; then
    echo
    echo "*** $not_measured row(s) NOT MEASURED: this machine could not supply the budget."
    echo "*** Those rows are not results. Rerun them with the memory free before reading"
    echo "*** this run as a measurement."
fi
