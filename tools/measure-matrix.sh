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
#   tools/measure-matrix.sh [conversation ...|all]
#
# Examples:
#   tools/measure-matrix.sh                 # the six heavy conversations, every profile
#   tools/measure-matrix.sh 368 631         # just these two
#   ENGINES=bwd tools/measure-matrix.sh 14    # one engine, one group
#   PROFILES=deepest-1 ENGINES=bwd tools/measure-matrix.sh 14   # one row
#   tools/measure-matrix.sh all             # EVERY group in the game, resumably
#
# `all` asks the measurement itself which groups exist - GROUPS_ONLY=1, one canonical start
# per distinct closure, heaviest first - rather than reading a list kept here, which could
# omit a group and never say so. It is 1,422 groups against the six a default run does.
#
# RESUMING. A run writes its rows as it finishes them, and pointing a later run at the same
# folder makes it skip what is already there:
#
#   MATRIX_OUT=measurements/logs/2026-09-07_whole-game tools/measure-matrix.sh all
#
# Run that again after a kill, a crash, or a reboot and it picks up where it stopped. It is
# the same command every time - there is no separate resume mode to remember, and no way to
# resume into the wrong folder by forgetting a flag. Without MATRIX_OUT each run gets its
# own folder, as before, and resumes nothing.
#
# WHAT COUNTS AS DONE, because the three outcomes are not alike:
#
#   ok            measured. Done.
#   CRASHED       the row took its process down. That IS the answer for that row, recorded
#                 as such, and a resume must not retry it for ever.
#   NO-ROWS       the measurement said there is nothing to measure - no group builds from
#                 this start, no entry 0, or nothing reachable. Also an answer, also done.
#   NOT-MEASURED  the machine could not supply the memory budget. NOTHING WAS MEASURED, so
#                 this is the one outcome a resume retries.
#
# THE ROW IN FLIGHT IS LOST, and that is accepted rather than overlooked. Recovering it
# would mean writing a marker before the row and reasoning about markers with no row after
# them; the cost of not doing it is one row out of fourteen thousand, re-measured.
#
# Rows are APPENDED as they finish, so a kill -9 keeps everything before it. A retried
# NOT-MEASURED row therefore leaves both lines in the file, in the order they happened;
# READ THE LAST ROW PER (conv, profile), which is what the resume itself does.
#
# ENGINES and PROFILES each take a comma or space separated list and narrow the grid the
# same way the conversation arguments do. The engines are fwd (the symbolic forward
# search), bwd (the backward one) and fwdbwd (the switching method the game actually runs:
# a forward slice, then the backward driver told what it found) - see the note at the
# top of measurements/performance_matrix.rs for what each is and how to read an older
# run, whose columns may spell two of these differently.
#
# STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL. Every row is a fresh cargo and
# a fresh measurement binary, so killing the terminal or the job leaves the script
# looping and starting new ones - which then hold that binary open and fail the next
# build with
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
#
# UNLESS THE RUN NAMES ONE, which is what makes a resume possible: the same folder, the
# same rows, the ones already in it skipped. A generated name cannot be resumed into
# because the next run generates a different one.
if [ -n "${MATRIX_OUT:-}" ]; then
    case "$MATRIX_OUT" in
        /*) LOGS="$MATRIX_OUT" ;;
        *) LOGS="$ROOT/$MATRIX_OUT" ;;
    esac
else
    LOGS="$(RUN_LOG_DIR="$OUT/logs" "$ROOT/tools/run-logged.sh" --folder-only measure matrix)"
fi
mkdir -p "$LOGS"

# WHAT WAS ASKED FOR, kept as given and resolved further down: `all` has to ask the
# measurement which groups exist, and the measurement is not built yet.
CONVERSATIONS=("$@")

# Counted so the run can say at the end that part of it is not a measurement. A line
# scrolled past an hour ago is not a warning.
not_measured=0

# Groups the measurement found nothing to measure in. Counted rather than warned about:
# over the whole game this is an ordinary and frequent outcome, and the number is worth
# seeing beside the rows that did measure something.
no_rows=0

# Every profile the measurement knows, unless a run names the ones it wants. A single
# row is a reasonable thing to ask for: the heavy groups spend the full cap per engine,
# so the whole grid is hours and one question is often one row.
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

# WHICH ENGINES EACH ROW MEASURES, passed through to it. Unset means all three - fwd,
# bwd, fwdbwd - which is what the grid is for; naming one or two is how a question
# about a single engine gets asked without paying for the others.
#
#   ENGINES=bwd tools/measure-matrix.sh 14
#
# EXPORTED ONLY WHEN IT HAS A VALUE. It reads an empty ENGINES as "all", so this is belt
# and braces - but an empty selection exported into a measurement is the kind of thing
# that should not have two chances to mean nothing.
if [ -n "${ENGINES:-}" ]; then
    export ENGINES
fi

# A row with no measurement in it, shaped by the header: the conversation and profile it
# was, the given verdict in every verdict column, and nothing claimed for the rest.
#
# TWO CALLERS, AND THEY MEAN OPPOSITE THINGS - CRASHED, the row took the process down, and
# NO-ROWS, the measurement looked and said there was nothing here. Sharing the shaping and
# not the verdict is what keeps them one line apart in the file.
verdict_row() {
    printf '%s' "$HEADER" | awk -F'\t' -v conv="$1" -v prof="$2" -v verdict="$3" '{
        for (i = 1; i <= NF; i++) {
            if ($i == "conv") cell = conv
            else if ($i == "profile") cell = prof
            else if ($i ~ /_verdict$/) cell = verdict
            else cell = "?"
            printf "%s%s", (i > 1 ? "\t" : ""), cell
        }
        printf "\n"
    }'
}

# THE CAP EACH ENGINE GETS, which the run needs a copy of to say anything about how long
# it has left. The measurement's own default is ten minutes (DEFAULT_ROW_SECONDS in
# measurements/performance_matrix.rs); this passes whatever is set through unchanged, and
# EACH ENGINE gets it separately, so a row's worst case is this times the number of
# engines measured, plus the build and the index read.
ROW_SECONDS="${ROW_SECONDS:-600}"
export ROW_SECONDS

DONE_ROWS=0
STARTED=$(date +%s)

# Rows this run did not measure because the folder already held them, and the seconds it
# spent on the ones it did. Both exist for the same reason: on a resume, elapsed time and
# rows finished stop being the same question.
SKIPPED_ROWS=0
SPENT=0

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
#
# A ROW THE RESUME SKIPPED passes "skipped": it advances the run but did not cost this run
# anything, so it must not enter the pace calibration. Feeding it in as zero seconds against
# its full weight drags the ratio down and the estimate with it - and on a resumed
# whole-game run the skipped rows are most of them.
#
# HOW OFTEN THE WEIGHTED ESTIMATE IS RECOMPUTED. Once a minute at most, because it re-reads
# every past TSV and is handed a spec naming every row still to come: at fourteen thousand
# rows that is real time, spent to refine a number nobody reads more than once a minute
# anyway. Between refreshes the last figure is reprinted.
ESTIMATE_EVERY=60
LAST_ESTIMATE=""
LAST_ESTIMATE_AT=0

progress() {
    DONE_ROWS=$(( DONE_ROWS + 1 ))
    local now
    now=$(date +%s)
    local elapsed=$(( now - STARTED ))
    local left=$(( TOTAL_ROWS - DONE_ROWS ))

    if [ "${1:-}" != "skipped" ]; then
        DONE_SPEC="${DONE_SPEC}${DONE_SPEC:+;}${ROW_KEYS[$(( DONE_ROWS - 1 ))]}=$(( now - ROW_STARTED ))"
        SPENT=$(( SPENT + now - ROW_STARTED ))
    fi

    local estimate="" note=""
    # The row just finished leaves the list of what is still to come. Stripped rather than
    # rebuilt: rebuilding it walks every remaining row, which is fine for sixty and is not
    # for fourteen thousand.
    case "$LEFT_SPEC" in
        *\;*) LEFT_SPEC="${LEFT_SPEC#*;}" ;;
        *) LEFT_SPEC="" ;;
    esac

    if [ "$left" -gt 0 ] && [ "${#PAST_TSVS[@]}" -gt 0 ] \
        && [ -n "$DONE_SPEC" ] && [ -n "$LEFT_SPEC" ] \
        && { [ -z "$LAST_ESTIMATE" ] || [ $(( now - LAST_ESTIMATE_AT )) -ge "$ESTIMATE_EVERY" ]; }
    then
        LAST_ESTIMATE="$(awk -v engines="$ENGINE_NAMES" -v done="$DONE_SPEC" -v left="$LEFT_SPEC" \
            -f "$ROOT/tools/matrix-remaining.awk" "${PAST_TSVS[@]}" 2>/dev/null)"
        LAST_ESTIMATE_AT=$now
    fi
    estimate="$LAST_ESTIMATE"

    # THE FLAT MEAN OVER WHAT THIS RUN ACTUALLY SPENT, not over its elapsed time: a resumed
    # run's elapsed clock includes rows it skipped in no time at all, and dividing that by
    # the rows it did would say a whole-game resume finishes this afternoon.
    if [ -z "$estimate" ]; then
        local measured=$(( DONE_ROWS - SKIPPED_ROWS ))
        if [ "$measured" -gt 0 ]; then
            estimate=$(( SPENT * left / measured ))
        else
            estimate=0
        fi
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
cargo build --release --example performance_matrix \
    --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1

# ASKED FOR RATHER THAN WRITTEN DOWN. The column names follow the engine selection, and a
# copy kept here would be wrong for any narrowed run and silently wrong for a renamed
# column - which is the mistake de-zovl exists to correct, in the one place it would still
# be possible to make.
HEADER="$(HEADER_ONLY=1 cargo run --release --quiet --example performance_matrix \
    --manifest-path "$ROOT/Cargo.toml" 2>/dev/null \
    | grep -m1 '^conv')"
if [ -z "$HEADER" ]; then
    echo "could not read the column names from the measurement - did the build fail?" >&2
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

# WHICH CONVERSATIONS, resolved here rather than at the top because `all` has to ask the
# measurement, and the measurement has only just been built.
#
# ASKED, NOT LISTED. The whole point of a whole-game run is that nothing decides which
# groups are in it except the index, so the enumeration comes from GROUPS_ONLY - one
# canonical start per distinct closure, heaviest first - and there is no list here to fall
# out of date. The default when nothing is named stays the six heavy conversations the
# matrix has always meant.
if [ ${#CONVERSATIONS[@]} -eq 1 ] && [ "${CONVERSATIONS[0]}" = "all" ]; then
    echo "asking the measurement which groups exist..."
    CONVERSATIONS=()
    while IFS=$'\t' read -r start _conversations _entries; do
        [ -n "$start" ] && CONVERSATIONS+=("$start")
    done < <(GROUPS_ONLY=1 cargo run --release --quiet --example performance_matrix \
        --manifest-path "$ROOT/Cargo.toml" 2>/dev/null)

    if [ ${#CONVERSATIONS[@]} -eq 0 ]; then
        echo "the measurement listed no groups - did the index read?" >&2
        exit 1
    fi
    echo "${#CONVERSATIONS[@]} groups"
elif [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(362 368 631 14 28 1030)
fi

TOTAL_ROWS=$(( ${#CONVERSATIONS[@]} * ${#PROFILES[@]} ))

# EVERY ROW THIS RUN WILL DO, in the order it will do them, so that at any point the run
# can say which rows are still ahead of it - which is what the weighted estimate needs and
# a count of rows cannot give. Must match the loop order below exactly.
ROW_KEYS=()
for conversation in "${CONVERSATIONS[@]}"; do
    for profile in "${PROFILES[@]}"; do
        ROW_KEYS+=("$conversation:$profile")
    done
done

# The same list as one string, which is what the estimator is handed. Built once here and
# shortened by a row at a time in `progress`.
LEFT_SPEC="$(IFS=';'; printf '%s' "${ROW_KEYS[*]}")"

# WHAT THIS FOLDER ALREADY HOLDS, which is the whole of the resume.
#
# A row is done if the folder has a line for it that is not NOT-MEASURED - see the header
# for why that one outcome is the exception. THE LAST LINE PER KEY WINS, because the files
# are appended to and a retried row sits after the one it replaces.
declare -A ROW_DONE=()
already=0
for tsv in "$LOGS"/performance-matrix-*.tsv; do
    [ -e "$tsv" ] || continue
    # conv, entries, profile, unseen, then the engine columns - ROW_COLUMNS in the
    # measurement, and the reason this reads a fourth field it does not use.
    while IFS=$'\t' read -r conv _entries profile rest; do
        case "$conv" in conv|"") continue ;; esac
        case "$rest" in
            *NOT-MEASURED*) unset "ROW_DONE[$conv:$profile]" ;;
            *) ROW_DONE["$conv:$profile"]=1 ;;
        esac
    done < "$tsv"
done
already=${#ROW_DONE[@]}

echo "$TOTAL_ROWS rows, ${ROW_SECONDS}s per engine per row, started $(date '+%H:%M:%S')"
if [ "$already" -gt 0 ]; then
    echo "resuming in $LOGS: $already row(s) already measured, and they will be skipped"
fi

for conversation in "${CONVERSATIONS[@]}"; do
    tsv="$LOGS/performance-matrix-$conversation.tsv"

    # ONLY WHEN THE FILE IS NEW. Truncating it here is what a resume must not do, and the
    # header is the one line that would otherwise be written twice.
    [ -e "$tsv" ] || echo "$HEADER" > "$tsv"
    echo "=== $conversation -> $tsv"

    for profile in "${PROFILES[@]}"; do
        log="$LOGS/matrix-$conversation-$profile.log"

        # ALREADY ANSWERED, so not asked again. Counted as done for the progress line, since
        # what the run has left is what it has left however the rows got there.
        if [ -n "${ROW_DONE[$conversation:$profile]:-}" ]; then
            printf '  %-12s  already measured\n' "$profile"
            ROW_STARTED=$(date +%s)
            SKIPPED_ROWS=$(( SKIPPED_ROWS + 1 ))
            progress skipped
            continue
        fi

        printf '  %-12s' "$profile"
        ROW_STARTED=$(date +%s)

        # CREATED EMPTY FIRST, because a row can produce NO output at all and the awk below
        # only creates the file on its first write. Under `cargo test` that could not happen
        # - the test harness always printed something - but a `cargo run` that exits without
        # printing leaves no log, and then the CRASHED message names a file that is not
        # there and `grep` says so. An empty log is the honest artefact of a row that said
        # nothing.
        : > "$log"

        # THE ROW LOG GETS EVERYTHING; THE RUN LOG GETS THE PROGRESS LINES.
        #
        # A heavy row is half an hour inside one cargo invocation, and redirecting it
        # wholesale to its own file - which is what this did - meant the only thing being
        # watched said nothing for half an hour while the interesting lines went somewhere
        # nobody was looking. Whether a row is watchable and whether its log is complete are
        # not the same question, so tee answers both: the file keeps the whole output, and
        # only the lines the measurement marks with the progress prefix reach stdout.
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
            cargo run --release --quiet --example performance_matrix \
            --manifest-path "$ROOT/Cargo.toml" 2>&1 \
            | awk -v rowlog="$log" '
                { print > rowlog; fflush(rowlog) }
                /^  ~/ { print; fflush() }
              ' || true
        status=${PIPESTATUS[0]}
        printf '  %-12s' "$profile"

        # EXIT 2 IS "YOU ASKED FOR SOMETHING THAT DOES NOT EXIST", and it stops the run.
        #
        # The measurement refuses an unknown profile or a conversation id that is not one,
        # rather than quietly selecting nothing (de-uxyw). Without this the refusal looked
        # exactly like a crash - no row line in the log - so a mistyped sixty-six row run
        # produced sixty-six CRASHED rows and took its several seconds over each of them.
        # There is nothing to measure and nothing to retry, so say what it said and stop.
        if [ "$status" -eq 2 ]; then
            echo "REFUSED"
            sed 's/^/  /' "$log"
            echo
            echo "nothing was measured; fix the selection and run again" >&2
            exit 2
        fi

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
        elif grep -q '; no rows$' "$log"; then
            # NOTHING TO MEASURE, AND THE MEASUREMENT SAID SO - no group builds from this
            # start, the group has no entry 0, or nothing is reachable from it. That is an
            # ANSWER about the group and not a death, and telling the two apart matters at
            # whole-game scale in a way it never did over six hand-picked conversations:
            # most of the 1,372 single-conversation groups are tiny and some of them are
            # empty, so reading these as CRASHED would fill the run with alarming rows that
            # mean "this group has no dialogue to search".
            verdict_row "$conversation" "$profile" NO-ROWS >> "$tsv"
            echo "no rows - $(grep -m1 '; no rows$' "$log")"
            no_rows=$((no_rows + 1))
        else
            # A CRASH IS A RESULT. The row says so and names its log, rather than being
            # silently absent - an empty line in a measurement reads as "not run yet",
            # which is a different thing from "this is what happens". Distinct from
            # NOT-MEASURED above: this row died, that one never ran. And distinct from
            # NO-ROWS: that one looked and found nothing, this one never came back.
            verdict_row "$conversation" "$profile" CRASHED >> "$tsv"
            echo "CRASHED (see $log)"
        fi
        progress
    done
done

echo
echo "wrote $(ls -1 "$LOGS"/performance-matrix-*.tsv | wc -l) file(s) in: $LOGS"

# NAMED ONE BY ONE ONLY WHEN THERE ARE FEW. A whole-game run writes fourteen hundred of
# them and the list is not a summary of anything.
if [ "$(ls -1 "$LOGS"/performance-matrix-*.tsv | wc -l)" -le 20 ]; then
    ls -1 "$LOGS"/performance-matrix-*.tsv
fi

if [ "$SKIPPED_ROWS" -gt 0 ]; then
    echo "$SKIPPED_ROWS row(s) were already in that folder and were skipped."
fi

if [ "$no_rows" -gt 0 ]; then
    echo "$no_rows row(s) had nothing to measure: no group, no entry 0, or nothing reachable."
fi

if [ "$not_measured" -gt 0 ]; then
    echo
    echo "*** $not_measured row(s) NOT MEASURED: this machine could not supply the budget."
    echo "*** Those rows are not results. Rerun them with the memory free before reading"
    echo "*** this run as a measurement. Re-running with the same MATRIX_OUT retries exactly"
    echo "*** those rows and leaves everything else alone."
fi
