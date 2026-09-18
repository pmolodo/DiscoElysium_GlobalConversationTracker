#!/usr/bin/env bash
# Run a symbolic-reachability measurement ONE CONVERSATION PER PROCESS.
#
# These measurements die in ways that take the whole process down: the diagram manager
# running out of nodes, a stack overflow inside a recursive diagram operation, a single
# step running minutes past its budget. Run several conversations in one process and the
# first crash destroys every row after it - a run that measured 368, then overflowed on
# 631, reported nothing at all for 14, 28 and 1030.
#
# One process each means a crash is a RESULT for that conversation and costs nothing else.
# Every run keeps its own folder under performance/logs, one log per conversation inside
# it, so a crashed row can still be read afterwards - and so can the run before this one.
#
# Usage:
#   tools/measure-symbolic.sh <measurement> [stage] [conversation ...]
#
# Examples:
#   tools/measure-symbolic.sh symbolic_answers
#   tools/measure-symbolic.sh shared_symbolic 631
#   tools/measure-symbolic.sh backward_support 368 631
#   tools/measure-symbolic.sh layout_shape slots 14
#
# THE MEASUREMENT IS AN EXAMPLE under performance/, named the way Cargo.toml names it.
# It used to be a test NAME plus a TEST_BINARY saying which binary to find it in; an
# example is its own binary, so the two collapsed into one argument. That pairing had also
# gone stale - its documented default named a test file that no longer exists, so running
# this with no arguments could not work at all.
#
# A measurement holding several stages behind one `main` - layout_shape is the one that
# does - takes the stage name as a second argument. Anything that parses as a number is
# read as a conversation, so the stage is optional and order still reads naturally.
set -u

MEASUREMENT="${1:-symbolic_answers}"
shift || true

# An argument that is not a number is the stage to run; conversations are numbers.
STAGE=""
if [ $# -gt 0 ] && ! printf '%s' "$1" | grep -qE '^[0-9]+$'; then
    STAGE="$1"
    shift
fi

# The five groups that drive the cost, unless told otherwise.
CONVERSATIONS=("$@")
if [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(368 631 14 28 1030)
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The prefix is applied by the helper rather than typed - see CLAUDE.md.
. "$ROOT/tools/degct-env.sh"

# ONE FOLDER PER RUN, and under performance/ rather than target/ - a `cargo clean` should
# not take measurements with it, and a run's logs only mean anything as a set. LOG_DIR
# still overrides the whole thing, which is what a one-off comparison wants.
DEGCT_RUN_NAME="${DEGCT_MEASUREMENT}${STAGE:+-$STAGE}"
DEGCT_LOG_DIR="$(degct_env LOG_DIR "$(DEGCT_RUN_LOG_DIR="$ROOT/performance/logs" \
    "$ROOT/tools/run-logged.sh" --folder-only measure "$DEGCT_RUN_NAME")")"
mkdir -p "$DEGCT_LOG_DIR"

echo "measuring ${DEGCT_RUN_NAME}, one process per conversation"
echo "logs in ${DEGCT_LOG_DIR}"
echo

# Build once, so a compile does not get charged to the first conversation's timing.
cargo build --release --example "${DEGCT_MEASUREMENT}" --quiet || exit 1

for conversation in "${CONVERSATIONS[@]}"; do
    log="${DEGCT_LOG_DIR}/${DEGCT_RUN_NAME}-${conversation}.log"
    echo "=== conversation ${conversation} ==="

    DEGCT_CONVERSATION="${conversation}" cargo run --release --quiet \
        --example "${DEGCT_MEASUREMENT}" ${STAGE:+-- "$STAGE"} >"${log}" 2>&1
    status=$?

    # A crash is a data point, not a reason to stop. Report how it died and carry on.
    if [ ${status} -ne 0 ]; then
        echo "  DIED (exit ${status}) - see ${log}"
        grep -E "overflowed its stack|OutOfMemory|panicked" "${log}" | head -3 | sed 's/^/  /'
    fi

    # The measurement's own rows, whatever happened after them.
    #
    # THE `===` HEADINGS COUNT AS ROWS. Several measurements say their finding on one -
    # backward_support's is "worst target 1030:359, largest set 649 diagram nodes" - and
    # without them a run of one of those printed nothing at all under the conversation it
    # had just measured, which reads as a row that produced no output rather than one this
    # summary does not know the shape of. The full log is still where everything is.
    grep -E "^ +[0-9]+ +[0-9]+|^=== |quarry|guards |\.\.\. " "${log}" | sed 's/^/  /'
    echo
done

echo "done; full output per conversation is in ${DEGCT_LOG_DIR}"
