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
# Every run keeps its own folder under measurements/logs, one log per conversation inside
# it, so a crashed row can still be read afterwards - and so can the run before this one.
#
# Usage:
#   tools/measure-symbolic.sh <test-name> [conversation ...]
#
# Examples:
#   tools/measure-symbolic.sh finding_one_unseen_entry_in_a_group_that_is_otherwise_seen
#   tools/measure-symbolic.sh what_the_expensive_conversations_cost 631
#   TEST_BINARY=backward_cost tools/measure-symbolic.sh what_one_backward_pass_costs
set -u

TEST_NAME="${1:-finding_one_unseen_entry_in_a_group_that_is_otherwise_seen}"
shift || true

# Which test binary the named test lives in. There is more than one measurement now, and
# they are not all in symbolic_reachability - the backward ones are in backward_cost.
TEST_BINARY="${TEST_BINARY:-symbolic_reachability}"

# The five groups that drive the cost, unless told otherwise.
CONVERSATIONS=("$@")
if [ ${#CONVERSATIONS[@]} -eq 0 ]; then
    CONVERSATIONS=(368 631 14 28 1030)
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ONE FOLDER PER RUN, and under measurements/ rather than target/ - a `cargo clean` should
# not take measurements with it, and a run's logs only mean anything as a set. LOG_DIR
# still overrides the whole thing, which is what a one-off comparison wants.
LOG_DIR="${LOG_DIR:-$(RUN_LOG_DIR="$ROOT/measurements/logs" \
    "$ROOT/tools/run-logged.sh" --folder-only measure "$TEST_NAME")}"
mkdir -p "$LOG_DIR"

echo "measuring ${TEST_NAME}, one process per conversation"
echo "logs in ${LOG_DIR}"
echo

# Build once, so a compile does not get charged to the first conversation's timing.
cargo build --release --tests --quiet || exit 1

for conversation in "${CONVERSATIONS[@]}"; do
    log="${LOG_DIR}/${TEST_NAME}-${conversation}.log"
    echo "=== conversation ${conversation} ==="

    CONVERSATION="${conversation}" cargo test --release \
        --test "${TEST_BINARY}" "${TEST_NAME}" \
        -- --ignored --nocapture --test-threads=1 >"${log}" 2>&1
    status=$?

    # A crash is a data point, not a reason to stop. Report how it died and carry on.
    if [ ${status} -ne 0 ]; then
        echo "  DIED (exit ${status}) - see ${log}"
        grep -E "overflowed its stack|OutOfMemory|panicked" "${log}" | head -3 | sed 's/^/  /'
    fi

    # The measurement's own rows, whatever happened after them.
    grep -E "^ +[0-9]+ +[0-9]+|quarry|guards |\.\.\. " "${log}" | sed 's/^/  /'
    echo
done

echo "done; full output per conversation is in ${LOG_DIR}"
