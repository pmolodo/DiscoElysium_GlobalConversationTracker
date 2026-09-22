#!/usr/bin/env bash
# Run a command and keep its whole output under the run-log name, in the tree its kind names.
#
# GameHarness logs itself - see tools/GameAutomation/RunLog.cs - so this is for the runs
# that are not ours to modify: cargo, dotnet test, and the measurement scripts.
#
# Usage:
#   tools/run-logged.sh [--kind <kind>] <tool> <verb> [--] <command> [args ...]
#   tools/run-logged.sh --name-only <tool> <verb>     # print the path, run nothing
#   tools/run-logged.sh --folder-only <tool> <verb>   # ditto, as a directory
#
# WHICH TREE A RUN'S LOG BELONGS TO, by --kind. EVERY RUN IS A MEASUREMENT, and the kind says
# WHAT IT MEASURES - which is why none of the three is called "measure":
#
#   performance  performance/logs  timing: numbers to compare against other numbers
#   testing      testing/logs      correctness: suites, in-game runs and builds
#   analysis     analysis/logs     data: what some other run decided, read back
#
# Each kind names its own tree, so a log's kind and its path say the same word.
#
# THERE IS NO DEFAULT. A tool may declare its own kind in a header line of its own - see
# `declared_kind` - and where neither says, the run is refused. The tool NAME cannot decide
# it: `cargo full-suite` measures correctness and `cargo walk-1467` measures timing, and both
# are cargo.
#
# RUN_LOG_DIR still overrides all of it, for a caller that wants a log somewhere else
# entirely.
#
# Examples:
#   tools/run-logged.sh tools/measure-menus.py all   # kind from the tool's own header
#   tools/run-logged.sh --kind performance cargo residue -- \
#     cargo run --release -p gct_measure --example search_residue
#   tools/run-logged.sh --kind testing cargo corpus -- cargo test --test suite corpus::
#   tools/run-logged.sh --kind testing dotnet unit -- dotnet test
#   DISCO_ELYSIUM_GCT_INGAME_TESTS=1 \
#     tools/run-logged.sh --kind testing dotnet in-game -- dotnet test tools/GameAutomation.Tests
#
# The name is <date>_<time>_<revision>_<tool>_<verb>, where the time is HH,MM,SS - commas
# because a Windows file name cannot hold a colon - with -dirty on the revision when the
# tree has been changed since the commit, and _2, _3 ... when even that name is taken,
# which takes two runs starting in the same second. THE FORMAT LIVES IN TWO PLACES, here
# and in RunLog.cs, because one of them has to work without a build and the other has to
# work without a shell; RunLogTests holds them to each other.
#
# The command's exit status is this script's exit status, so it drops into a pipeline
# where the bare command stood.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# READ THROUGH THE HELPER, not by name. Every environment variable this project defines is
# DEGCT_ prefixed and the prefix is applied by `degct_env` rather than typed - see CLAUDE.md
# for the rule and docs/environment.md for the list.
. "$ROOT/tools/degct-env.sh"

# WHICH TREE, from --kind, read before the options below so the rest can use it.
KIND=""
if [ "${1:-}" = "--kind" ]; then
    KIND="${2:-}"
    shift 2
fi

# WHAT A TOOL SAYS ABOUT ITSELF. A tool's kind is a property of the tool, not of each
# incantation, so a script declares it once in its own header:
#
#     # run-log-kind: analysis
#
# and every call of it lands in the right tree without anyone remembering. --kind still wins,
# for a tool used for two purposes.
#
# NOTHING IS GUESSED. Where no --kind is given and no script in the command declares one -
# `cargo test`, `dotnet run`, anything that is not a file we can read - the run is REFUSED
# rather than defaulted. A default is how a test suite's transcript ended up filed with the
# measurements for months: the wrong answer was silent, and the right one nobody typed.
declared_kind() {
    local argument found
    for argument in "$@"; do
        [ -f "$argument" ] || continue
        found="$(sed -n 's/^#[[:space:]]*run-log-kind:[[:space:]]*\([a-z]*\).*/\1/p' \
            "$argument" 2>/dev/null | head -1)"
        if [ -n "$found" ]; then
            printf '%s' "$found"
            return
        fi
    done
}

# The kind's tree unless a caller says otherwise, so that a run's raw output stays beside
# what it produced without every incantation having to say so.
#
# UNDER A FOLDER PER DATE, because one directory of every run ever taken is one nobody
# browses: it reached 1,669 transcripts and 179 row folders before this, at which point
# finding the run from a particular afternoon meant reading a wall of names and shell
# completion was useless. The date is already the first field of every name, so grouping by it
# costs nothing and loses nothing - and tools/tidy-logs.py sorts any that arrive loose.
#
# Called once the command is known, since that is what carries the script whose header may
# declare the kind. RUN_LOG_DIR skips the question entirely: a caller naming the folder has
# already answered it.
set_log_dir() {
    local root
    if [ -n "$(degct_env RUN_LOG_DIR "")" ]; then
        DEGCT_LOG_DIR="$(degct_env RUN_LOG_DIR "")/$(date +%Y-%m-%d)"
        return
    fi
    # A KIND IS ITS TREE'S NAME, so there is nothing to map and nothing to keep in step.
    case "$1" in
        performance | testing | analysis) root="$ROOT/$1/logs" ;;
        "")
            echo "run-logged.sh: no --kind, and nothing in the command declares one." >&2
            echo "Pass --kind performance|testing|analysis, or give the tool a header line:" >&2
            echo "    # run-log-kind: analysis" >&2
            exit 2
            ;;
        *)
            echo "--kind $1: expected performance, testing or analysis" >&2
            exit 2
            ;;
    esac
    DEGCT_LOG_DIR="$root/$(date +%Y-%m-%d)"
}

usage() {
    sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

# One name component, with anything awkward taken out of it - the same rule RunLog.Safe
# applies: letters, digits, dash and dot survive, everything else becomes a dash.
safe() {
    printf '%s' "$1" | tr -c 'A-Za-z0-9.-' '-'
}

# What the tree is sitting at: the commit, plus -dirty when it has moved on. "nogit" when
# git cannot say, rather than no log at all - see the remarks on RunLog.Revision.
# SHORT IN THE NAME, FULL IN THE FILE. A forty-character hash in every log name made the
# names too wide to read at a glance and too wide to elide safely - the middle is the part a
# reader would cut, and the middle is the hash. Seven characters identify a commit in this
# repository unambiguously, and the full one is printed in the transcript's own header, where
# nothing is competing for the width.
revision() {
    local head
    head="$(git -C "$ROOT" rev-parse --short=7 HEAD 2>/dev/null)" || { printf 'nogit'; return; }
    if [ -z "$head" ]; then
        printf 'nogit'
        return
    fi
    if [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ]; then
        printf '%s-dirty' "$head"
    else
        printf '%s' "$head"
    fi
}

# The whole hash, for the transcript header. "nogit" where git cannot say, as `revision` does.
full_revision() {
    git -C "$ROOT" rev-parse HEAD 2>/dev/null || printf 'nogit'
}

# The stem every run-log name is built from.
stem() {
    printf '%s_%s_%s_%s_%s' \
        "$(date +%Y-%m-%d)" "$(date +%H,%M,%S)" "$(revision)" \
        "$(safe "$1")" "$(safe "$2")"
}

# The given path, or the first numbered variant of it that is free. Two runs at one commit
# on one day is the normal case, and the second must not overwrite the first.
unique() {
    local base="$1" extension="$2" candidate attempt
    candidate="${base}${extension}"
    attempt=2
    while [ -e "$candidate" ]; do
        candidate="${base}_${attempt}${extension}"
        attempt=$((attempt + 1))
    done
    printf '%s' "$candidate"
}

case "${1:-}" in
    --name-only)
        [ $# -eq 3 ] || usage
        # NAMING A PATH RUNS NOTHING, so there is no command to read a kind from - a caller
        # asking for a name either said --kind or set RUN_LOG_DIR, which is what every
        # in-repository caller does.
        set_log_dir "${KIND:-performance}"
        unique "$DEGCT_LOG_DIR/$(stem "$2" "$3")" ".txt"
        echo
        exit 0
        ;;
    --folder-only)
        [ $# -eq 3 ] || usage
        set_log_dir "${KIND:-performance}"
        unique "$DEGCT_LOG_DIR/$(stem "$2" "$3")" ""
        echo
        exit 0
        ;;
    -h|--help|"")
        usage
        ;;
esac

[ $# -ge 3 ] || usage

TOOL="$1"
VERB="$2"
shift 2
[ "${1:-}" = "--" ] && shift
[ $# -ge 1 ] || usage

# THE COMMAND IS KNOWN NOW, so a tool that declares its own kind can be asked.
[ -n "$KIND" ] || KIND="$(declared_kind "$@")"
set_log_dir "$KIND"

mkdir -p "$DEGCT_LOG_DIR"
LOG="$(unique "$DEGCT_LOG_DIR/$(stem "$TOOL" "$VERB")" ".txt")"

{
    printf '# %s\n' "$*"
    printf '# revision %s\n\n' "$(full_revision)"
} > "$LOG"

echo "logging to $LOG"

# WHY THESE TWO ARE STILL ENVIRONMENT VARIABLES - de-3dx9, and this is the ruling
#
# Every option this project takes is a CLI argument, because an argument documents itself and
# --help is never stale. These two cannot be, and the reason is what this script IS: it wraps
# a command it does not parse and has never heard of. `cargo test --release`, `dotnet test`,
# `python tools/measure-menus.py all` - adding a flag to somebody else's command line is not a
# thing a wrapper may do, and which flag would even be right depends on the wrapped tool.
#
# THREE ALTERNATIVES, NAMED RATHER THAN LEFT UNCONSIDERED:
#
#   PASS A FLAG. The same impossibility said twice: to know the flag, the wrapper would have
#   to know the tool, and its whole value is that it does not.
#
#   WRITE THEM TO A FILE AND PASS THE PATH. Passing the path is passing a flag, so this only
#   moves the problem, and it adds a file to clean up.
#
#   LET THE TOOL ASK THIS SCRIPT. It already can, and does: `measurement_common.folder_for`
#   shells out to `--folder-only` when nothing wrapped it. But a tool that ASKED for the name
#   would derive a second one - the wrapper names a run for the instant it started, and the
#   tool asking a moment later gets a different instant, so the transcript and the rows would
#   carry different names and the pairing this exists to create would be exactly what broke.
#   The value has to be the one already decided, which means handing it over.
#
# So they stay, and the shape is the narrow one: a wrapper hands DOWN what it alone knows.
# Nothing sets them by hand, and a person who does is overriding the wrapper on purpose.
#
# WHICH TRANSCRIPT A FOLDER OF ROWS BELONGS TO. A run writes two things in two places - this
# transcript, named for when it ran, and a folder of rows named for what it measured - and
# only one direction was findable: the transcript prints where the rows went, and the rows
# said nothing about the transcript. A driver records every DEGCT_ variable into its run.json,
# so exporting the path is all it takes to close the loop.
#
# NOT COMPARED ON A RESUME, because `settings_of` matches only the variables in
# COMPARED_VARIABLES - which this is deliberately not one of. A resumed folder would otherwise
# refuse itself, its second invocation having a different transcript by construction.
export DEGCT_RUN_LOG="$LOG"

# WHAT THE RUN IS FOR, which decides more than where the log lands. A cold throw-away pass
# protects a comparison between timings; a test and an analysis pass produce neither, so they
# skip it and cost half as much. The driver cannot work this out for itself - the same
# measure-menus.py invocation is a measurement or a dataset derivation depending only on why
# it was asked for - so the kind is passed down rather than guessed.
export DEGCT_RUN_KIND="$KIND"

# tee, not a plain redirect, so a long run can still be watched while it runs. Its own
# exit status is what matters, not tee's, hence PIPESTATUS.
"$@" 2>&1 | tee -a "$LOG"
status="${PIPESTATUS[0]}"

echo "logged to $LOG"
exit "$status"
