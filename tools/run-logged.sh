#!/usr/bin/env bash
# Run a command and keep its whole output in measurements/logs, under the run-log name.
#
# GameHarness logs itself - see tools/GameAutomation/RunLog.cs - so this is for the runs
# that are not ours to modify: cargo, dotnet test, and the measurement scripts.
#
# Usage:
#   tools/run-logged.sh <tool> <verb> [--] <command> [args ...]
#   tools/run-logged.sh --name-only <tool> <verb>     # print the path, run nothing
#   tools/run-logged.sh --folder-only <tool> <verb>   # ditto, as a directory
#
# RUN_LOG_DIR overrides where logs are written; it defaults to measurements/logs, which is
# where the slow runs that most want a log already put theirs.
#
# Examples:
#   tools/run-logged.sh cargo shared-symbolic -- cargo run --release --example shared_symbolic
#   tools/run-logged.sh cargo corpus -- cargo test --test corpus
#   tools/run-logged.sh dotnet unit -- dotnet test
#   DISCO_ELYSIUM_GCT_INGAME_TESTS=1 \
#     tools/run-logged.sh dotnet in-game -- dotnet test tools/GameAutomation.Tests
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

# measurements/logs unless a caller says otherwise, so that a measurement's raw output
# stays beside the rows it produced without every incantation having to say so.
DEGCT_LOG_DIR="$(degct_env RUN_LOG_DIR "$ROOT/measurements/logs")"

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
revision() {
    local head
    head="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)" || { printf 'nogit'; return; }
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
        unique "$DEGCT_LOG_DIR/$(stem "$2" "$3")" ".txt"
        echo
        exit 0
        ;;
    --folder-only)
        [ $# -eq 3 ] || usage
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

mkdir -p "$DEGCT_LOG_DIR"
LOG="$(unique "$DEGCT_LOG_DIR/$(stem "$TOOL" "$VERB")" ".txt")"

{
    printf '# %s\n\n' "$*"
} > "$LOG"

echo "logging to $LOG"

# tee, not a plain redirect, so a long run can still be watched while it runs. Its own
# exit status is what matters, not tee's, hence PIPESTATUS.
"$@" 2>&1 | tee -a "$LOG"
status="${PIPESTATUS[0]}"

echo "logged to $LOG"
exit "$status"
