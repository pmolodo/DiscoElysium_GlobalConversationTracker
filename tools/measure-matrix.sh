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
# AND IT ASKS WHICH OF THEM HAVE ANYTHING IN THEM. 901 of the 1,422 reach nothing from
# their start, mostly the two-entry ORB stubs the database is full of; they are recorded as
# NO-ROWS from the enumeration, which costs a third of a second for the whole game, instead
# of by nine thousand processes that each build a graph to find the same nothing. See the
# pruning below for why that answer is asked for and not cached.
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
# STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL. Every row is a fresh measurement
# process, so killing the terminal or the job leaves the script
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

# WHICH ENGINES EACH ROW MEASURES, passed through to it. Unset means fwdbwd alone since
# de-8xcd - the engine the game runs and the one being tuned - because fwd and bwd cost
# several times what it does and are evidence rather than products. `ENGINES=all` measures
# the three, which is what the grid is for; naming one or two asks a question about a single
# engine without paying for the others.
#
#   ENGINES=bwd tools/measure-matrix.sh 14
#   ENGINES=all tools/measure-matrix.sh 14
#
# EXPORTED ONLY WHEN IT HAS A VALUE. The measurement reads an empty ENGINES as the default,
# so this is belt and braces - but an empty selection exported into a measurement is the
# kind of thing that should not have two chances to mean nothing.
if [ -n "${ENGINES:-}" ]; then
    export ENGINES
fi

TAB=$'\t'

# A row with no measurement in it, shaped by the header: the conversation and profile it
# was, the given verdict in every verdict column, and nothing claimed for the rest.
#
# THREE CALLERS, AND THEY MEAN DIFFERENT THINGS - CRASHED, the row took the process down;
# NO-ROWS, a row process looked and said there was nothing here; and NO-ROWS again for a
# group the enumeration pruned before any process ran. Sharing the shaping and not the
# verdict is what keeps them one line apart in the file.
#
# IN THE SHELL RATHER THAN IN AWK, which it used to be, AND LEFT IN A VARIABLE RATHER THAN
# PRINTED. The pruning below writes nine thousand of these in one go: an awk apiece is nine
# thousand processes, and `$(...)` around a shell function is nine thousand forks, which on
# Windows is minutes of nothing but process creation. `ROW_LINE` is read by the caller.
#
# The loop is over the header's fields, so it still follows a narrowed run's columns rather
# than assuming a shape.
verdict_row() {
    local conv="$1" prof="$2" verdict="$3" name cell
    ROW_LINE=""
    for name in "${HEADER_FIELDS[@]}"; do
        case "$name" in
            conv) cell="$conv" ;;
            profile) cell="$prof" ;;
            *_verdict) cell="$verdict" ;;
            *) cell="?" ;;
        esac
        if [ -z "$ROW_LINE" ]; then ROW_LINE="$cell"; else ROW_LINE="$ROW_LINE$TAB$cell"; fi
    done
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

# AND THEN CALLED DIRECTLY, not through `cargo run`.
#
# `cargo run` re-checks the build on every invocation, which is work the line above has
# just done. Measured 2026-09-08 on an up-to-date tree, asking only for the header so the
# measuring itself is nil: 0.552s through cargo against 0.042s for the binary. That is
# ~510ms on every row, and a whole-game run is fourteen thousand of them - about two hours
# spent re-answering one question.
#
# IT IS ALSO A LOCK, which matters for anything that wants to run rows side by side:
# concurrent `cargo run`s serialise on the target directory.
#
# CHECKED ONCE, HERE. A missing binary called directly gives a shell error per row, and
# every one of those would be recorded as a crashed row - a build failure written into the
# folder as fourteen thousand findings.
MEASUREMENT="${CARGO_TARGET_DIR:-$ROOT/target}/release/examples/performance_matrix"
[ -x "$MEASUREMENT" ] || MEASUREMENT="$MEASUREMENT.exe"
if [ ! -x "$MEASUREMENT" ]; then
    echo "the measurement did not build - no runnable binary at $MEASUREMENT" >&2
    exit 1
fi

# ASKED FOR RATHER THAN WRITTEN DOWN. The column names follow the engine selection, and a
# copy kept here would be wrong for any narrowed run and silently wrong for a renamed
# column - which is the mistake de-zovl exists to correct, in the one place it would still
# be possible to make.
HEADER="$(HEADER_ONLY=1 "$MEASUREMENT" 2>/dev/null | grep -m1 '^conv')"
if [ -z "$HEADER" ]; then
    echo "could not read the column names from the measurement - did the build fail?" >&2
    exit 1
fi

IFS="$TAB" read -r -a HEADER_FIELDS <<< "$HEADER"

# WHICH engines a row measures, and how many, counted from the header rather than from a
# second reading of ENGINES: one verdict column each, whatever the selection was.
ENGINE_NAMES=$(printf '%s' "$HEADER" | tr '\t' '\n' | sed -n 's/_verdict$//p' | paste -sd, -)
ENGINE_COUNT=$(printf '%s' "$HEADER" | tr '\t' '\n' | grep -c '_verdict$')

# WHETHER ANY ENGINE REPORTS WHAT IT HELD. The `_nodes` columns are the manager's own node
# count, which is memory in use in the currency the budget is spent in - and they are the
# only evidence the split below has that a group would fit a worker's divided share. A run
# narrowed to `ENGINES=fwdbwd` has no such column, and the split says so rather than
# assuming the memory is fine because it cannot see any.
REPORTS_NODES=$(printf '%s' "$HEADER" | tr '\t' '\n' | grep -c '_nodes$')

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
#
# AND THE ENUMERATION SAYS WHICH GROUPS HAVE ANYTHING IN THEM, which is the fourth column
# and the reason most of a whole-game run no longer happens. 901 of the 1,422 groups reach
# nothing from their start - nearly all of them the two-entry ORB stubs the database is
# full of - and measuring one meant ten processes that each read the index, built the same
# graph, found the same nothing and said so. The enumeration answers that for every group
# in the game in about a third of a second, because it has the index open already and the
# question is one walk per group.
EMPTY_GROUPS=()
if [ ${#CONVERSATIONS[@]} -eq 1 ] && [ "${CONVERSATIONS[0]}" = "all" ]; then
    echo "asking the measurement which groups exist..."
    CONVERSATIONS=()
    while IFS="$TAB" read -r start _conversations _entries reachable; do
        [ -n "$start" ] || continue
        # A MISSING COLUMN IS A STALE BINARY, not an empty group, and the difference is the
        # whole run: read as zero it would prune every group in the game and record the
        # lot as NO-ROWS in seconds. The script and the measurement are built together, so
        # this can only mean the build did not take.
        if [ -z "$reachable" ]; then
            echo "the measurement's group list has no 'reachable' column - it is older than" >&2
            echo "this script. Rebuild it and run again." >&2
            exit 1
        fi
        if [ "$reachable" -gt 0 ]; then
            CONVERSATIONS+=("$start")
        else
            EMPTY_GROUPS+=("$start")
        fi
    done < <(GROUPS_ONLY=1 "$MEASUREMENT" 2>"$LOGS/groups.log")

    if [ $(( ${#CONVERSATIONS[@]} + ${#EMPTY_GROUPS[@]} )) -eq 0 ]; then
        echo "the measurement listed no groups - did the index read?" >&2
        exit 1
    fi
    echo "$(( ${#CONVERSATIONS[@]} + ${#EMPTY_GROUPS[@]} )) groups, ${#CONVERSATIONS[@]} of them with rows"
elif [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(362 368 631 14 28 1030)
fi

# WHAT THIS FOLDER ALREADY HOLDS, which is the whole of the resume.
#
# A row is done if the folder has a line for it that is not NOT-MEASURED - see the header
# for why that one outcome is the exception. THE LAST LINE PER KEY WINS, because the files
# are appended to and a retried row sits after the one it replaces.
#
# READ BEFORE ANYTHING IS WRITTEN, which the pruning below depends on: a group recorded as
# NO-ROWS on the last run must not have ten more NO-ROWS rows appended to it on this one.
declare -A ROW_DONE=()
for tsv in "$LOGS"/performance-matrix-*.tsv; do
    [ -e "$tsv" ] || continue
    # conv, entries, profile, unseen, then the engine columns - ROW_COLUMNS in the
    # measurement, and the reason this reads a fourth field it does not use.
    while IFS="$TAB" read -r conv _entries profile rest; do
        case "$conv" in conv|"") continue ;; esac
        case "$rest" in
            *NOT-MEASURED*) unset "ROW_DONE[$conv:$profile]" ;;
            *) ROW_DONE["$conv:$profile"]=1 ;;
        esac
    done < "$tsv"
done

# THE EMPTY GROUPS, RECORDED WITHOUT RUNNING ANYTHING.
#
# Pruned is not the same as forgotten. The folder still gets a TSV per group with a
# NO-ROWS row per profile, exactly as it did when a process wrote each one, so nothing
# downstream can tell the difference and a group is never silently absent from a whole-game
# folder. What is gone is the nine thousand processes.
#
# WHY THIS IS NOT A CACHED LIST. It was worth asking - the answer is the same every time
# the index is - but the enumeration costs a third of a second for the whole game and a
# committed list of empty groups would be a second copy of the index's own shape, wrong
# and silent the first time a group grew an entry. The measurement is asked, like the group
# list and the column names before it.
#
# The reasons are in groups.log, one line per pruned group, in the wording the row logs
# used: "conversation 1500: nothing is reachable from its start; no rows".
if [ ${#EMPTY_GROUPS[@]} -gt 0 ]; then
    pruned_rows=0
    for conversation in "${EMPTY_GROUPS[@]}"; do
        tsv="$LOGS/performance-matrix-$conversation.tsv"
        [ -e "$tsv" ] || echo "$HEADER" > "$tsv"
        block=""
        for profile in "${PROFILES[@]}"; do
            [ -n "${ROW_DONE[$conversation:$profile]:-}" ] && continue
            verdict_row "$conversation" "$profile" NO-ROWS
            block="$block$ROW_LINE"$'\n'
            pruned_rows=$(( pruned_rows + 1 ))
            no_rows=$(( no_rows + 1 ))
        done
        [ -n "$block" ] && printf '%s' "$block" >> "$tsv"
    done
    if [ "$pruned_rows" -gt 0 ]; then
        echo "${#EMPTY_GROUPS[@]} group(s) reach nothing from their start: $pruned_rows row(s) recorded as NO-ROWS without running one"
    else
        echo "${#EMPTY_GROUPS[@]} group(s) reach nothing from their start, and the folder already records every one of them"
    fi
    echo "  the reason for each is in $LOGS/groups.log"
fi

TOTAL_ROWS=$(( ${#CONVERSATIONS[@]} * ${#PROFILES[@]} ))

# EVERY ROW THIS RUN WILL DO, in the order it will do them, so that at any point the run
# can say which rows are still ahead of it - which is what the weighted estimate needs and
# a count of rows cannot give. Must match the loop order below exactly.
#
# `already` IS COUNTED HERE rather than from the size of ROW_DONE, which now also holds the
# pruned groups' rows: what the resume message is about is the work this run was going to
# do and will not.
ROW_KEYS=()
already=0
for conversation in "${CONVERSATIONS[@]}"; do
    for profile in "${PROFILES[@]}"; do
        ROW_KEYS+=("$conversation:$profile")
        [ -n "${ROW_DONE[$conversation:$profile]:-}" ] && already=$(( already + 1 ))
    done
done

# The same list as one string, which is what the estimator is handed. Built once here and
# shortened by a row at a time in `progress`.
LEFT_SPEC="$(IFS=';'; printf '%s' "${ROW_KEYS[*]}")"

echo "$TOTAL_ROWS rows, ${ROW_SECONDS}s per engine per row, started $(date '+%H:%M:%S')"
if [ "$already" -gt 0 ]; then
    echo "resuming in $LOGS: $already row(s) already measured, and they will be skipped"
fi

# Set by the phase drivers at the bottom. `measure_group` is the same code either way and
# only the accounting around it differs.
IN_PARALLEL=0

# ONE ROW IS DONE.
#
# In the serial phase that is the progress line and its weighted estimate, exactly as
# before. In the parallel phase it is a tally kept by the worker, because THE ESTIMATOR'S
# PREMISE IS ONE ROW AT A TIME, TIMED: with four rows in flight, "row 0:00:02" is no
# longer a duration anybody waited and a remaining-time weighted by past row costs is no
# longer being divided by the right pace. The parallel phase reports by GROUP instead and
# says so, rather than printing a number it cannot stand behind.
row_finished() {
    if [ "$IN_PARALLEL" = 1 ]; then
        GROUP_ROWS=$(( GROUP_ROWS + 1 ))
    else
        progress "$@"
    fi
}

# ONE GROUP, START TO FINISH. Lifted out of the loop it used to be so that the parallel
# phase below can run several at once; the body is otherwise what it always was, one
# process per row.
#
# IT COUNTS ITS OWN OUTCOMES rather than adding to the run's totals, because in the
# parallel phase it runs in a forked subshell and anything it added to a total would be
# added to a copy that dies with the fork. The caller folds the GROUP_ counters in -
# directly when it ran here, through a file when it ran in a worker.
#
# SKIPPED_ROWS IS THE ONE EXCEPTION and is incremented in both places: the serial progress
# line needs it live, within the group, to know what this run actually measured, so the
# fold cannot wait until the group ends. In the parallel phase that increment lands in the
# fork and is discarded, and GROUP_SKIPPED is what the parent folds instead.
measure_group() {
    local conversation="$1"
    local tsv log row status profile
    GROUP_NOT_MEASURED=0
    GROUP_NO_ROWS=0
    GROUP_SKIPPED=0
    GROUP_ROWS=0

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
            # NO CLOCK, DELIBERATELY: nothing ran, so there is no start to report. The space
            # where one would go is held open so this line stays in the same column as the
            # rows that did run.
            printf '  %8s  %-18s already measured\n' "" "$profile"
            ROW_STARTED=$(date +%s)
            SKIPPED_ROWS=$(( SKIPPED_ROWS + 1 ))
            GROUP_SKIPPED=$(( GROUP_SKIPPED + 1 ))
            row_finished skipped
            continue
        fi

        # BOTH CLOCKS BEFORE THE PRINT, so the time shown is when the row started rather
        # than a moment after it. Two `date` calls rather than converting the epoch one:
        # `date -d @...` is GNU-only and this script runs under Git Bash on Windows.
        ROW_STARTED=$(date +%s)
        ROW_CLOCK=$(date +%H:%M:%S)

        # THE WALL CLOCK IS ON THIS LINE, THE ONE THAT EXISTS WHILE THE ROW IS RUNNING.
        # de-p58a. The progress line carries durations only - "row 0:11:31 elapsed 0:54:37" -
        # all relative to a start nobody wrote down, so a reader could not say when a row
        # began and, for the row in flight, could not say anything at all. Putting it on the
        # verdict line instead would only be readable once the row had finished, by which
        # time the duration is printed anyway and the question has answered itself.
        #
        # Colons are fine here. run-logged.sh writes HH,MM,SS in FILE NAMES because a Windows
        # file name cannot hold a colon; that constraint does not apply to log content.
        printf '  %s  %-18s ...' "$ROW_CLOCK" "$profile"

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
            "$MEASUREMENT" 2>&1 \
            | awk -v rowlog="$log" '
                { print > rowlog; fflush(rowlog) }
                /^  ~/ { print; fflush() }
              ' || true
        status=${PIPESTATUS[0]}

        # THE SEPARATOR IS PRINTED, NOT IMPLIED. de-qm7a: this used to be '  %-12s' and let
        # the field padding supply the gap before the verdict. Every profile that existed
        # when it was written fits in twelve - "deepest-1" and "95pc-seen" are both nine - so
        # there was always padding and the gap looked like part of the format. Then
        # "deepest-unreach-1" arrived at seventeen, the field overflowed, no padding was
        # emitted, and the log said "deepest-unreach-1ok". Widening alone would leave the
        # same trap for the next longer name, so the width is for ALIGNMENT and the trailing
        # space is for correctness.
        #
        # THE SAME START CLOCK AS THE LINE ABOVE, not the finish time: it is what pairs the
        # two lines, which matters in the parallel phase where several workers interleave in
        # one log. The duration is already on the progress line.
        printf '  %s  %-18s ' "$ROW_CLOCK" "$profile"

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
                    GROUP_NOT_MEASURED=$((GROUP_NOT_MEASURED + 1))
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
            verdict_row "$conversation" "$profile" NO-ROWS
            echo "$ROW_LINE" >> "$tsv"
            echo "no rows - $(grep -m1 '; no rows$' "$log")"
            GROUP_NO_ROWS=$((GROUP_NO_ROWS + 1))
        else
            # A CRASH IS A RESULT. The row says so and names its log, rather than being
            # silently absent - an empty line in a measurement reads as "not run yet",
            # which is a different thing from "this is what happens". Distinct from
            # NOT-MEASURED above: this row died, that one never ran. And distinct from
            # NO-ROWS: that one looked and found nothing, this one never came back.
            verdict_row "$conversation" "$profile" CRASHED
            echo "$ROW_LINE" >> "$tsv"
            echo "CRASHED (see $log)"
        fi
        row_finished
    done
}

# THE HEAVY GROUPS ONE AT A TIME, THEN THE REST SEVERAL AT A TIME - AND THE RUN DECIDES
# WHERE THAT IS FROM WHAT IT HAS JUST MEASURED.
#
# It used to be a number: the first twenty-five groups serially, the other 1,397 in
# parallel, because on the run of 2026-09-07 the last row to take more than ten seconds was
# in group 26. That number is a property of one measurement of one index on one machine.
# Every one of those can move - a change to the search, a group that grows entries, a
# machine with more cores and so a smaller share of the budget each - and when it does the
# constant is silently in the wrong place, in the direction that matters: a heavy group
# measured in parallel gets a DIVIDED budget and a contended clock, which is a row that
# looks like a finding and is an artefact.
#
# So the run watches two metrics per group and switches when both say the tail has arrived.
#
# 1. THE TIME HAS BOTTOMED OUT. Groups arrive heaviest-first, so the cost falls and then
#    flattens; what "flat" means is measured against the run's own cheapest group so far,
#    not against a number of seconds. A group counts as settled when its recorded search
#    time is within SETTLE_FACTOR of that floor.
#
# 2. WHAT IT HELD FITS THE CAP A WORKER WILL GET, comfortably. Each parallel worker is
#    allowed FULL_BUDGET_MB/WORKERS, so the question is not "did this fit six gigabytes"
#    but "would it have fitted a quarter of them" - and with MEMORY_HEADROOM to spare,
#    because the groups being cleared for are the ones AFTER this one, which nothing has
#    measured yet.
#
# BOTH, FOR SETTLE_GROUPS GROUPS IN A ROW, and one that fails either resets the count. The
# curve is not monotone: over the whole game the ten groups after the seven heavy ones look
# like the tail, and then 825, 362, 1030 and 625 arrive - the last of them 47s and a
# gigabyte, at group 26. A window of one would have handed all four to the workers; ten in
# a row does not switch until group 35, after which the heaviest thing left in the game is
# 15s and 33 MB, or two per cent of a worker's cap. That is the whole margin this buys, and
# it costs ten cheap groups measured one at a time.
#
# WHY RECORDED SEARCH TIME AND NOT THE CLOCK: see tools/matrix-group-cost.awk. A resume
# skips most of its rows, and a stopwatch cannot tell "cheap" from "already done".
#
# SETTLE_GROUPS=n, SETTLE_FACTOR=n and MEMORY_HEADROOM=n move the rule; SERIAL_GROUPS=n
# replaces it with the old fixed count, which is how a run that has to be comparable with
# an existing folder asks for one. WORKERS=1 never switches at all.
SETTLE_GROUPS="${SETTLE_GROUPS:-10}"
SETTLE_FACTOR="${SETTLE_FACTOR:-2}"
MEMORY_HEADROOM="${MEMORY_HEADROOM:-2}"
SERIAL_GROUPS="${SERIAL_GROUPS:-}"
WORKERS="${WORKERS:-$(nproc 2>/dev/null || printenv NUMBER_OF_PROCESSORS || echo 1)}"

# 6144 mirrors DiagramBudget::measurement() and 40 mirrors
# DiagramBudget::BYTES_PER_NODE, both in src/symbolic/budget.rs. They are the two numbers
# here that have to be kept in step with the Rust by hand: the first is what a row is
# allowed, the second is what the `_nodes` columns have to be multiplied by to be in the
# same currency as it.
FULL_BUDGET_MB="${ROW_MEMORY_MB:-6144}"
BYTES_PER_NODE=40
WORKER_MB=$(( FULL_BUDGET_MB / WORKERS ))
WORKER_NODES=$(( WORKER_MB * 1024 * 1024 / BYTES_PER_NODE ))
FITS_NODES=$(( WORKER_NODES / MEMORY_HEADROOM ))

# GROUPS RUN IN PARALLEL, NEVER ROWS, so no two workers ever touch one file: a group owns
# its performance-matrix-<start>.tsv. The append-as-it-finishes resume needs no locking and
# no changes, and rows within a group stay in their own order.
#
# WHY THE SPLIT CANNOT BE DECIDED UP FRONT any more, and what is lost by that: the run can
# no longer say at the start how many groups go each way. It says what it is watching for
# instead, and says the moment it switches.
if [ "$WORKERS" -le 1 ]; then
    echo "one worker: every group one at a time"
elif [ -n "$SERIAL_GROUPS" ]; then
    echo "$SERIAL_GROUPS group(s) one at a time, then $WORKERS at a time (SERIAL_GROUPS was set)"
elif [ "$REPORTS_NODES" -eq 0 ]; then
    # No `_nodes` column, so nothing here can say what a group held, and clearing a group
    # for a quarter of the budget on no evidence is exactly the mistake the headroom exists
    # to avoid. Refusing to switch is slow; switching blind manufactures rows.
    echo "no engine in this selection reports nodes held, so nothing can say whether a group"
    echo "would fit a worker's share of the budget: every group one at a time. Name"
    echo "SERIAL_GROUPS=n to split anyway."
else
    echo "one group at a time until the cost bottoms out: $SETTLE_GROUPS in a row within"\
" ${SETTLE_FACTOR}x the cheapest group so far, each holding at most $FITS_NODES nodes"
    echo "  (1/${MEMORY_HEADROOM} of the $WORKER_NODES a worker's ${WORKER_MB} MB share of the ${FULL_BUDGET_MB} MB budget buys)"
fi

# How many groups have gone serially, which is where the parallel phase picks up.
serial_done=0

# The cheapest group this run has seen, and how many since have been settled. Empty until
# the first group with a cost in it.
floor_ms=""
settled=0
window_max_ms=0
window_max_nodes=0

for conversation in "${CONVERSATIONS[@]}"; do
    measure_group "$conversation"
    not_measured=$(( not_measured + GROUP_NOT_MEASURED ))
    no_rows=$(( no_rows + GROUP_NO_ROWS ))
    serial_done=$(( serial_done + 1 ))

    if [ "$WORKERS" -le 1 ]; then
        continue
    fi

    if [ -n "$SERIAL_GROUPS" ]; then
        [ "$serial_done" -ge "$SERIAL_GROUPS" ] && break
        continue
    fi

    [ "$REPORTS_NODES" -eq 0 ] && continue

    # WHAT THIS GROUP COST, off its own file, so a group the resume skipped still counts.
    read -r group_ms group_nodes group_complete \
        < <(awk -f "$ROOT/tools/matrix-group-cost.awk" \
            "$LOGS/performance-matrix-$conversation.tsv" 2>/dev/null)
    group_ms="${group_ms:-0}"
    group_nodes="${group_nodes:-0}"
    group_complete="${group_complete:-0}"

    if [ "$group_complete" != "1" ]; then
        # A group with a crashed, unmeasured or empty row in it is not evidence that the
        # measuring has got cheap - it is evidence that something did not measure.
        settled=0
        window_max_ms=0
        window_max_nodes=0
        continue
    fi

    if [ -z "$floor_ms" ] || [ "$group_ms" -lt "$floor_ms" ]; then
        floor_ms="$group_ms"
    fi

    if [ "$group_ms" -le $(( floor_ms * SETTLE_FACTOR )) ] \
        && [ "$group_nodes" -le "$FITS_NODES" ]
    then
        settled=$(( settled + 1 ))
        [ "$group_ms" -gt "$window_max_ms" ] && window_max_ms="$group_ms"
        [ "$group_nodes" -gt "$window_max_nodes" ] && window_max_nodes="$group_nodes"
    else
        settled=0
        window_max_ms=0
        window_max_nodes=0
    fi

    if [ "$settled" -ge "$SETTLE_GROUPS" ]; then
        echo "  cost has bottomed out after $serial_done group(s): the last $SETTLE_GROUPS"\
" spent at most ${window_max_ms}ms of search against a floor of ${floor_ms}ms,"
        echo "  and held at most $window_max_nodes nodes of the $WORKER_NODES a worker gets."\
" The rest go $WORKERS at a time."
        break
    fi
done

parallel_groups=("${CONVERSATIONS[@]:$serial_done}")

if [ "${#parallel_groups[@]}" -gt 0 ]; then
    IN_PARALLEL=1
    PARALLEL_STARTED=$(date +%s)
    GROUPS_DONE=0

    # EACH WORKER GETS ITS SHARE OF THE ALLOWANCE, and this is a correctness fix rather
    # than tidiness. The manager PREALLOCATES about two thirds of the budget up front and
    # cannot grow past it (src/symbolic/budget.rs:280-286), so a 43-entry tail group
    # commits roughly four gigabytes exactly as a heavy one does - nothing about a small
    # group makes it cheaper. Four workers at the 6 GB default would commit ~16 GB before
    # measuring anything, and each one's can_be_supplied() probe fallibly reserves the FULL
    # 6 GB first. Overlap those and the run manufactures NOT-MEASURED rows where a probe
    # was refused and CRASHED rows where a probe passed and the allocation aborted - the
    # race named at budget.rs:300 and performance_matrix.rs:1169.
    #
    # THE FOLDER IS TOLD WHICH ALLOWANCE THESE ROWS GOT. Two rows given different budgets
    # are not comparable (performance_matrix.rs:328-330) and nothing in a TSV records the
    # budget, so it is written down here instead. A `no-room` row measured under a divided
    # budget is SUSPECT and wants re-running serially at the full allowance before it is
    # believed: no-room is meant to say the search had every byte it was allowed, not that
    # it was allowed a quarter of them.
    #
    # WORKER_MB IS WHAT THE SPLIT ABOVE CLEARED EACH GROUP AGAINST, so it is the same
    # figure rather than a second division of the same budget.
    export ROW_MEMORY_MB="$WORKER_MB"
    echo "each worker is allowed ${ROW_MEMORY_MB} MB of the ${FULL_BUDGET_MB} MB budget"
    printf '%s\n' \
        "workers=$WORKERS budget_mb=$ROW_MEMORY_MB of=$FULL_BUDGET_MB groups=${#parallel_groups[@]} started=$(date '+%F %T')" \
        >> "$LOGS/parallel-phase.txt"

    # WHERE THE RUN IS, once rows stop arriving one at a time. The estimate is flat and
    # needs no scaling by the worker count: wall time per finished group already has the
    # concurrency inside it.
    group_finished() {
        GROUPS_DONE=$(( GROUPS_DONE + 1 ))
        local now elapsed left
        now=$(date +%s)
        elapsed=$(( now - PARALLEL_STARTED ))
        left=$(( ${#parallel_groups[@]} - GROUPS_DONE ))
        printf '    group %d/%d  elapsed %s  est. left ~%s\n' \
            "$GROUPS_DONE" "${#parallel_groups[@]}" \
            "$(clock "$elapsed")" \
            "$(clock $(( elapsed * left / GROUPS_DONE )))"
    }

    # A worker's output is held and printed whole when its group finishes, so that four
    # groups at once do not interleave into something nobody can read.
    WORK="$LOGS/.workers"
    mkdir -p "$WORK"
    declare -A WORKER_OF=()
    running=0

    reap() {
        local pid group
        wait -n -p pid || true
        [ -n "${pid:-}" ] || return 0
        group="${WORKER_OF[$pid]:-}"
        [ -n "$group" ] || return 0
        unset "WORKER_OF[$pid]"
        [ -e "$WORK/$group.out" ] && cat "$WORK/$group.out"
        rm -f "$WORK/$group.out"
        # WRITTEN LAST BY THE WORKER, so its absence means the worker itself died rather
        # than the row. The rows it did finish are already in the TSV - they are appended
        # as they happen - so this loses only the tally, and saying so beats adding zero.
        if [ -e "$WORK/$group.stat" ]; then
            # shellcheck disable=SC1090
            . "$WORK/$group.stat"
            not_measured=$(( not_measured + GROUP_NOT_MEASURED ))
            no_rows=$(( no_rows + GROUP_NO_ROWS ))
            SKIPPED_ROWS=$(( SKIPPED_ROWS + GROUP_SKIPPED ))
            rm -f "$WORK/$group.stat"
        else
            echo "WORKER LOST for group $group - its finished rows are in the TSV, its tally is not"
        fi
        group_finished
    }

    for conversation in "${parallel_groups[@]}"; do
        while [ "$running" -ge "$WORKERS" ]; do
            reap
            running=$(( running - 1 ))
        done
        (
            measure_group "$conversation" > "$WORK/$conversation.out" 2>&1
            printf 'GROUP_NOT_MEASURED=%d\nGROUP_NO_ROWS=%d\nGROUP_SKIPPED=%d\n' \
                "$GROUP_NOT_MEASURED" "$GROUP_NO_ROWS" "$GROUP_SKIPPED" \
                > "$WORK/$conversation.stat"
        ) &
        WORKER_OF[$!]="$conversation"
        running=$(( running + 1 ))
    done

    while [ "$running" -gt 0 ]; do
        reap
        running=$(( running - 1 ))
    done
    rmdir "$WORK" 2>/dev/null || true
fi

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
