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
#   ENGINES=bwd tools/measure-matrix.sh 14    # one engine, one group
#   PROFILES=deepest-1 ENGINES=bwd tools/measure-matrix.sh 14   # one row
#
# ENGINES and PROFILES each take a comma or space separated list and narrow the grid the
# same way the conversation arguments do. The engines are fwd (the symbolic forward
# search), bwd (the backward one) and fwdbwd (the switching method the game actually runs:
# a forward slice, then the backward driver told what it found) - see the note at the top
# of tests/performance_matrix.rs for what each is and how to read an older run, whose
# columns may spell two of these differently.
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

# Every profile the test knows, unless a run names the ones it wants. A single row is a
# reasonable thing to ask for: the heavy groups spend the full cap per engine, so the
# whole grid is hours and one question is often one row.
if [ -n "${PROFILES:-}" ]; then
    IFS=', ' read -r -a PROFILES <<< "$PROFILES"
else
    PROFILES=(
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
fi

# WHICH ENGINES EACH ROW MEASURES, passed through to the test. Unset means all three -
# fwd, bwd, fwdbwd - which is what the grid is for; naming one or two is how a question
# about a single engine gets asked without paying for the others.
#
#   ENGINES=bwd tools/measure-matrix.sh 14
#
# EXPORTED ONLY WHEN IT HAS A VALUE. The test reads an empty ENGINES as "all", so this is
# belt and braces - but an empty selection exported into a measurement is the kind of thing
# that should not have two chances to mean nothing.
if [ -n "${ENGINES:-}" ]; then
    export ENGINES
fi

# A row that died, shaped by the header: the conversation and profile it was, CRASHED in
# every verdict column, and nothing claimed for the rest.
crashed_row() {
    printf '%s' "$HEADER" | awk -F'\t' -v conv="$1" -v prof="$2" '{
        for (i = 1; i <= NF; i++) {
            if ($i == "conv") cell = conv
            else if ($i == "profile") cell = prof
            else if ($i ~ /_verdict$/) cell = "CRASHED"
            else cell = "?"
            printf "%s%s", (i > 1 ? "\t" : ""), cell
        }
        printf "\n"
    }'
}

# THE CAP EACH ENGINE GETS, which the run needs a copy of to say anything about how long
# it has left. The test's own default is ten minutes (DEFAULT_ROW_SECONDS in
# tests/performance_matrix.rs); this passes whatever is set through unchanged, and EACH
# ENGINE gets it separately, so a row's worst case is this times the number of engines
# measured, plus the build and the index read.
ROW_SECONDS="${ROW_SECONDS:-600}"
export ROW_SECONDS

TOTAL_ROWS=$(( ${#CONVERSATIONS[@]} * ${#PROFILES[@]} ))
DONE_ROWS=0
STARTED=$(date +%s)

# EVERY ROW THIS RUN WILL DO, in the order it will do them, so that at any point the run
# can say which rows are still ahead of it - which is what the weighted estimate needs and
# a count of rows cannot give. Must match the loop order below exactly.
ROW_KEYS=()
for conversation in "${CONVERSATIONS[@]}"; do
    for profile in "${PROFILES[@]}"; do
        ROW_KEYS+=("$conversation:$profile")
    done
done

# The rows already finished, as key=seconds, accumulated as the run goes.
DONE_SPEC=""

# h:mm:ss. A run of this length is watched rather than read afterwards, and seconds since
# the epoch is not something a person can watch.
clock() {
    printf '%d:%02d:%02d' $(( $1 / 3600 )) $(( ($1 % 3600) / 60 )) $(( $1 % 60 ))
}

# WHERE THE RUN IS, after every row.
#
# Two numbers rather than one, because they bracket an honest answer and neither does it
# alone. The estimate is WEIGHTED BY WHAT EACH REMAINING ROW HAS COST BEFORE, taken from
# past runs under measurements/logs and scaled by the pace this run is actually going at -
# see tools/matrix-remaining.awk. The worst case is every remaining row spending every
# engine's cap in full, which is the number that says whether this can possibly finish
# overnight.
#
# THE FLAT MEAN IS STILL THE FALLBACK, and says so when it is used. It reads LONG early on,
# because each conversation's heavy profiles run first, and that is the whole reason the
# weights are worth having.
progress() {
    DONE_ROWS=$(( DONE_ROWS + 1 ))
    local now
    now=$(date +%s)
    local elapsed=$(( now - STARTED ))
    local left=$(( TOTAL_ROWS - DONE_ROWS ))

    DONE_SPEC="${DONE_SPEC}${DONE_SPEC:+;}${ROW_KEYS[$(( DONE_ROWS - 1 ))]}=$(( now - ROW_STARTED ))"

    local left_spec="" i
    for (( i = DONE_ROWS; i < TOTAL_ROWS; i++ )); do
        left_spec="${left_spec}${left_spec:+;}${ROW_KEYS[$i]}"
    done

    local estimate="" note=""
    if [ -n "$left_spec" ] && [ "${#PAST_TSVS[@]}" -gt 0 ]; then
        estimate="$(awk -v engines="$ENGINE_NAMES" -v done="$DONE_SPEC" -v left="$left_spec" \
            -f "$ROOT/tools/matrix-remaining.awk" "${PAST_TSVS[@]}" 2>/dev/null)"
    fi
    if [ -z "$estimate" ]; then
        estimate=$(( elapsed * left / DONE_ROWS ))
        note=" (flat)"
    fi

    printf '    %d/%d (%d%%)  row %s  elapsed %s  est. left ~%s%s  worst case %s\n' \
        "$DONE_ROWS" "$TOTAL_ROWS" $(( DONE_ROWS * 100 / TOTAL_ROWS )) \
        "$(clock $(( now - ROW_STARTED )))" \
        "$(clock "$elapsed")" \
        "$(clock "$estimate")" "$note" \
        "$(clock $(( left * ENGINE_COUNT * ROW_SECONDS )))"
}

# Built once, up front. Letting each row build would put a compile inside the timing of
# whichever row happened to run first.
echo "building..."
cargo build --release --tests --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1

# ASKED FOR RATHER THAN WRITTEN DOWN. The column names follow the engine selection, and a
# copy kept here would be wrong for any narrowed run and silently wrong for a renamed
# column - which is the mistake de-zovl exists to correct, in the one place it would still
# be possible to make.
HEADER="$(HEADER_ONLY=1 cargo test --release --test performance_matrix \
    --manifest-path "$ROOT/Cargo.toml" -- --ignored --nocapture 2>/dev/null \
    | grep -m1 '^conv')"
if [ -z "$HEADER" ]; then
    echo "could not read the column names from the test - did the build fail?" >&2
    exit 1
fi

# WHICH engines a row measures, and how many, counted from the header rather than from a
# second reading of ENGINES: one verdict column each, whatever the selection was.
ENGINE_NAMES=$(printf '%s' "$HEADER" | tr '\t' '\n' | sed -n 's/_verdict$//p' | paste -sd, -)
ENGINE_COUNT=$(printf '%s' "$HEADER" | tr '\t' '\n' | grep -c '_verdict$')

# WHAT PAST RUNS COST, for the weighted estimate. This run's own folder is excluded: its
# rows are the ones being calibrated, and letting them weigh themselves would drag every
# ratio towards one as the run went on.
PAST_TSVS=()
while IFS= read -r tsv; do
    [ -n "$tsv" ] && PAST_TSVS+=("$tsv")
done < <(find "$OUT/logs" -name 'performance-matrix-*.tsv' -not -path "$LOGS/*" 2>/dev/null)

echo "$TOTAL_ROWS rows, ${ROW_SECONDS}s per engine per row, started $(date '+%H:%M:%S')"

for conversation in "${CONVERSATIONS[@]}"; do
    tsv="$LOGS/performance-matrix-$conversation.tsv"
    echo "$HEADER" > "$tsv"
    echo "=== $conversation -> $tsv"

    for profile in "${PROFILES[@]}"; do
        log="$LOGS/matrix-$conversation-$profile.log"
        printf '  %-12s' "$profile"
        ROW_STARTED=$(date +%s)

        # THE ROW LOG GETS EVERYTHING; THE RUN LOG GETS THE PROGRESS LINES.
        #
        # A heavy row is half an hour inside one cargo invocation, and redirecting it
        # wholesale to its own file - which is what this did - meant the only thing being
        # watched said nothing for half an hour while the interesting lines went somewhere
        # nobody was looking. Whether a row is watchable and whether its log is complete are
        # not the same question, so tee answers both: the file keeps the whole output, and
        # only the lines the test marks with the progress prefix come through to stdout.
        #
        # AWK RATHER THAN `tee | grep --line-buffered`, which does both jobs in one process
        # and flushes explicitly after every line it passes on.
        #
        # A NOTE ON WHY, BECAUSE THE OBVIOUS EXPLANATION IS WRONG. The `tee | grep` version
        # was written first and, in one observed run, delivered nothing to the watched log
        # for eighty seconds while the row's own file already held three progress lines.
        # That looks exactly like tee block-buffering its stdout into a pipe, and this
        # comment said so. It is not: feeding that pipeline one marked line a second and
        # stamping each on arrival shows every line coming through in the second it was
        # written, with tee and grep in place. So the cause of that run's silence is NOT
        # established, and nothing here should be read as evidence about tee.
        #
        # What is established is that this version delivers. It is kept because it works and
        # because one process with an explicit flush leaves less to be wrong about, not
        # because the alternative was proven guilty.
        printf '\n'
        CONVERSATION="$conversation" PROFILE="$profile" NO_HEADER=1 \
            cargo test --release --test performance_matrix \
            --manifest-path "$ROOT/Cargo.toml" \
            -- --ignored --nocapture 2>&1 \
            | awk -v rowlog="$log" '
                { print > rowlog; fflush(rowlog) }
                /^  ~/ { print; fflush() }
              ' || true
        printf '  %-12s' "$profile"

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
            crashed_row "$conversation" "$profile" >> "$tsv"
            echo "CRASHED (see $log)"
        fi
        progress
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
