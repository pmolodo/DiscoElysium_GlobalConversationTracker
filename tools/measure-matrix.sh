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
# Writes one TSV per conversation into measurements/, plus one folder per run under
# measurements/logs holding a log per row.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/measurements"

# ONE FOLDER PER RUN, named for the day and the commit it measured - the whole matrix is
# one measurement, and its sixty-six row logs only mean anything as a set. Flat files
# named for the row were overwritten by the next run, which left every recorded TSV with
# no logs behind it except the newest one's.
LOGS="$(RUN_LOG_DIR="$OUT/logs" "$ROOT/tools/run-logged.sh" --folder-only measure matrix)"
mkdir -p "$LOGS"

CONVERSATIONS=("$@")
if [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(362 368 631 14 28 1030)
fi

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
    tsv="$OUT/performance-matrix-$conversation.tsv"
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
            echo "ok"
        else
            # A CRASH IS A RESULT. The row says so and names its log, rather than being
            # silently absent - an empty line in a measurement reads as "not run yet",
            # which is a different thing from "this is what happens".
            echo -e "$conversation\t?\t$profile\t?\tCRASHED\t?\t?\tCRASHED\t?\t?\t?" >> "$tsv"
            echo "CRASHED (see $log)"
        fi
    done
done

echo
echo "wrote:"
ls -1 "$OUT"/performance-matrix-*.tsv
echo "logs for this run: $LOGS"
